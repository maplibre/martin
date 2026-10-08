use std::fmt::{self, Display};
use std::ops::Range;
use std::pin::Pin;
use std::task::{Context, Poll};

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, Stream, StreamExt as _};
use object_store::path::Path;
use object_store::{
    CopyOptions, GetOptions, GetResult, GetResultPayload, ListResult, MultipartUpload, ObjectMeta,
    ObjectStore, PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions, Result,
};
use tracing::{Span, instrument};

#[derive(Debug)]
pub(crate) struct LabeledStore {
    inner: Box<dyn ObjectStore>,
    source: String,
}

impl LabeledStore {
    pub(crate) fn new(inner: Box<dyn ObjectStore>, source: impl Into<String>) -> Self {
        Self {
            inner,
            source: source.into(),
        }
    }

    fn in_span<'a, T: Send + 'a>(&self, stream: BoxStream<'a, T>) -> BoxStream<'a, T> {
        InSpan {
            inner: stream,
            span: tracing::info_span!("object_store", source = %self.source),
        }
        .boxed()
    }
}

impl Display for LabeledStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

struct InSpan<'a, T> {
    inner: BoxStream<'a, T>,
    span: Span,
}

impl<T> Stream for InSpan<'_, T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let span = self.span.clone();
        let _entered = span.enter();
        self.inner.poll_next_unpin(cx)
    }
}

#[async_trait]
impl ObjectStore for LabeledStore {
    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %location))]
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> Result<PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %location))]
    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %location))]
    async fn get_opts(&self, location: &Path, options: GetOptions) -> Result<GetResult> {
        let mut result = self.inner.get_opts(location, options).await?;
        if let GetResultPayload::Stream(stream) = result.payload {
            result.payload = GetResultPayload::Stream(self.in_span(stream));
        }
        Ok(result)
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %location))]
    async fn get_ranges(&self, location: &Path, ranges: &[Range<u64>]) -> Result<Vec<Bytes>> {
        self.inner.get_ranges(location, ranges).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, Result<Path>>,
    ) -> BoxStream<'static, Result<Path>> {
        self.in_span(self.inner.delete_stream(locations))
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, Result<ObjectMeta>> {
        self.in_span(self.inner.list(prefix))
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, Result<ObjectMeta>> {
        self.in_span(self.inner.list_with_offset(prefix, offset))
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source))]
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %from))]
    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<()> {
        self.inner.copy_opts(from, to, options).await
    }

    #[instrument(name = "object_store", skip_all, fields(source = %self.source, path = %from))]
    async fn rename_opts(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<()> {
        self.inner.rename_opts(from, to, options).await
    }
}

#[cfg(test)]
mod tests {
    use object_store::memory::InMemory;
    use tracing_test::traced_test;

    use super::*;

    fn store() -> LabeledStore {
        LabeledStore::new(Box::new(InMemory::new()), "tiles")
    }

    #[test]
    fn display_is_the_inner_store() {
        assert_eq!(store().to_string(), InMemory::new().to_string());
    }

    #[traced_test]
    #[tokio::test]
    async fn stream_polls_run_inside_the_source_span() {
        let stream = futures::stream::once(async {
            tracing::info!("polled");
            1
        })
        .boxed();

        let items: Vec<i32> = store().in_span(stream).collect().await;

        assert_eq!(items, [1]);
        assert!(logs_contain("object_store{source=tiles}"));
    }
}
