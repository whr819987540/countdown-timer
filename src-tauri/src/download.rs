//! 分段并行下载：把文件切成几段，用多个 HTTP/1.1 连接同时下载后拼接，
//! 绕开单连接限速（部分网络下 GitHub 发布资源的单连接速度很慢）。
//!
//! - 先用 `Range: bytes=0-0` 探测文件大小；服务器不支持范围请求时退化为单连接下载；
//! - 某段出错或连接卡住时从已收到的位置续传，每段最多尝试 [`MAX_ATTEMPTS`] 次；
//! - 这里只保证字节数正确，内容是否完整可信由调用方校验更新包签名来保证。

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::future::try_join_all;
use reqwest::header::{HeaderMap, CONTENT_RANGE, RANGE};
use reqwest::{Client, ClientBuilder, Response, StatusCode, Url};

/// 最多同时使用的连接数
const MAX_SEGMENTS: u64 = 4;
/// 每段至少这么大：文件小时少开连接，省去多余的握手
const MIN_SEGMENT: u64 = 256 * 1024;
/// 每段最多尝试次数（含首次）
const MAX_ATTEMPTS: u32 = 3;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 连接超过这么久收不到数据就视为卡住，断开后续传
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// 下载用的客户端配置：只用 HTTP/1.1（HTTP/2 会把各段复用到同一个连接上，失去分段的意义），并设置超时。
pub fn client_builder() -> ClientBuilder {
    Client::builder()
        .http1_only()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
}

/// 下载 `url` 的完整内容。
///
/// 每收到一块数据调用一次 `on_progress(已下载字节数, 总字节数)`，总字节数未知时为 `None`。
/// 各段在同一个任务里并发推进，回调不会被同时调用，进度单调递增。
pub async fn fetch<F>(
    client: &Client,
    url: &Url,
    headers: &HeaderMap,
    on_progress: F,
) -> Result<Vec<u8>, String>
where
    F: Fn(u64, Option<u64>) + Sync,
{
    let probe = request(client, url, headers, Some(0..1)).await?;
    let total = match probe.status() {
        StatusCode::PARTIAL_CONTENT => content_range(&probe)
            .filter(|served| served.start == 0)
            .and_then(|served| served.total),
        // 服务器忽略了 Range，返回的就是完整文件
        StatusCode::OK => return read_all(probe, &on_progress).await,
        status => return Err(format!("服务器返回 {status}")),
    };
    let Some(total) = total else {
        // 不知道文件大小，无法切分
        let response = request(client, url, headers, None).await?;
        if !response.status().is_success() {
            return Err(format!("服务器返回 {}", response.status()));
        }
        return read_all(response, &on_progress).await;
    };
    let source = Source {
        client,
        url,
        // 跳转后的最终地址：各段直接请求它，省去一次跳转
        resolved: probe.url().clone(),
        headers,
        total,
    };
    drop(probe);

    let received = AtomicU64::new(0);
    let report = |len: usize| {
        let len = len as u64;
        let done = received.fetch_add(len, Ordering::Relaxed) + len;
        on_progress(done, Some(total));
    };
    let segments = split(total, MAX_SEGMENTS, MIN_SEGMENT)
        .into_iter()
        .map(|range| fetch_segment(&source, range, &report));
    // 任何一段最终失败时，其余各段随之取消
    Ok(try_join_all(segments).await?.concat())
}

/// 分段下载共用的请求参数
struct Source<'a> {
    client: &'a Client,
    url: &'a Url,
    resolved: Url,
    headers: &'a HeaderMap,
    total: u64,
}

async fn request(
    client: &Client,
    url: &Url,
    headers: &HeaderMap,
    range: Option<Range<u64>>,
) -> Result<Response, String> {
    let mut request = client.get(url.clone()).headers(headers.clone());
    if let Some(range) = range {
        // HTTP 的范围包含结束位置
        request = request.header(RANGE, format!("bytes={}-{}", range.start, range.end - 1));
    }
    request.send().await.map_err(|err| err.to_string())
}

/// 单连接读完整个响应体
async fn read_all(
    mut response: Response,
    on_progress: &impl Fn(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let total = response.content_length();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        bytes.extend_from_slice(&chunk);
        on_progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

/// 下载一段，出错时从已收到的位置续传
async fn fetch_segment(
    source: &Source<'_>,
    range: Range<u64>,
    report: &impl Fn(usize),
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut attempt = 1;
    loop {
        // 跳转得到的地址可能带有效期（GitHub 是几分钟），重试时从原始地址重新跳转
        let url = if attempt == 1 {
            &source.resolved
        } else {
            source.url
        };
        let start = range.start + bytes.len() as u64;
        match fetch_range(source, url, start..range.end, &mut bytes, report).await {
            Ok(()) => return Ok(bytes),
            Err(err) if attempt < MAX_ATTEMPTS => {
                eprintln!("分段 {range:?} 第 {attempt} 次下载失败，续传：{err}");
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

/// 请求 `range` 并把收到的数据追加到 `bytes`；中途出错时已收到的部分保留在 `bytes` 中
async fn fetch_range(
    source: &Source<'_>,
    url: &Url,
    range: Range<u64>,
    bytes: &mut Vec<u8>,
    report: &impl Fn(usize),
) -> Result<(), String> {
    if range.is_empty() {
        return Ok(());
    }
    let mut response = request(source.client, url, source.headers, Some(range.clone())).await?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        return Err(format!("范围请求返回 {}", response.status()));
    }
    // 文件大小变了说明服务器上的文件已被替换，不能和其他段拼在一起
    let matches = content_range(&response).is_some_and(|served| {
        served.start == range.start && served.end < range.end && served.total == Some(source.total)
    });
    if !matches {
        return Err("服务器返回的范围与请求不符".into());
    }
    let mut remaining = range.end - range.start;
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        let len = chunk.len() as u64;
        if len > remaining {
            return Err("服务器返回的数据超出请求范围".into());
        }
        remaining -= len;
        bytes.extend_from_slice(&chunk);
        report(chunk.len());
    }
    if remaining > 0 {
        return Err("连接提前断开".into());
    }
    Ok(())
}

/// `Content-Range` 响应头：`end` 包含在内，`total` 未知时为空
#[derive(Debug, PartialEq, Eq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: Option<u64>,
}

fn content_range(response: &Response) -> Option<ContentRange> {
    parse_content_range(response.headers().get(CONTENT_RANGE)?.to_str().ok()?)
}

/// 解析 `bytes <start>-<end>/<total>`，`total` 可以是 `*`
fn parse_content_range(value: &str) -> Option<ContentRange> {
    let (range, total) = value.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse().ok()?;
    let end = end.parse().ok()?;
    let total = match total {
        "*" => None,
        total => Some(total.parse().ok()?),
    };
    if start > end || total.is_some_and(|total| end >= total) {
        return None;
    }
    Some(ContentRange { start, end, total })
}

/// 把 `[0, total)` 切成连续的若干段：段数不超过 `max_segments`，且尽量让每段不小于 `min_segment`
fn split(total: u64, max_segments: u64, min_segment: u64) -> Vec<Range<u64>> {
    let count = total
        .div_ceil(min_segment.max(1))
        .clamp(1, max_segments.max(1));
    let size = total.div_ceil(count);
    (0..count)
        .map(|i| (i * size).min(total)..((i + 1) * size).min(total))
        .filter(|range| !range.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_file_uses_one_segment() {
        assert_eq!(split(1000, 4, 4096), vec![0..1000]);
        assert_eq!(split(4096, 4, 4096), vec![0..4096]);
    }

    #[test]
    fn segment_count_grows_with_size_up_to_limit() {
        assert_eq!(split(4097, 4, 4096), vec![0..2049, 2049..4097]);
        assert_eq!(split(100, 4, 10), vec![0..25, 25..50, 50..75, 75..100]);
    }

    #[test]
    fn segments_cover_file_without_gaps() {
        for total in [1, 9, 10, 11, 1_432_517, 7_000_003] {
            let ranges = split(total, 4, 256 * 1024);
            assert!(!ranges.is_empty() && ranges.len() <= 4);
            assert_eq!(ranges.first().map(|r| r.start), Some(0));
            assert_eq!(ranges.last().map(|r| r.end), Some(total));
            assert!(ranges.windows(2).all(|w| w[0].end == w[1].start));
            assert!(ranges.iter().all(|r| !r.is_empty()));
        }
    }

    #[test]
    fn empty_file_has_no_segments() {
        assert!(split(0, 4, 4096).is_empty());
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(
            parse_content_range("bytes 0-0/1432517"),
            Some(ContentRange {
                start: 0,
                end: 0,
                total: Some(1_432_517)
            })
        );
        assert_eq!(
            parse_content_range("bytes 100-199/*"),
            Some(ContentRange {
                start: 100,
                end: 199,
                total: None
            })
        );
    }

    #[test]
    fn rejects_invalid_content_range() {
        for value in [
            "",
            "bytes */1000",
            "bytes 0-0",
            "items 0-0/10",
            "bytes 5-4/10",
            "bytes 0-10/10",
            "bytes -1-2/10",
            "bytes a-b/10",
        ] {
            assert_eq!(parse_content_range(value), None, "{value}");
        }
    }
}
