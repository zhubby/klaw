use crate::{
    VoiceError,
    model::{AudioByteStream, SttSegment, TranscriptStream},
};
use bytes::Bytes;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

pub fn byte_stream_from_receiver(
    rx: mpsc::UnboundedReceiver<Result<Bytes, VoiceError>>,
) -> AudioByteStream {
    Box::pin(UnboundedReceiverStream::new(rx))
}

pub fn transcript_stream_from_receiver(
    rx: mpsc::UnboundedReceiver<Result<SttSegment, VoiceError>>,
) -> TranscriptStream {
    Box::pin(UnboundedReceiverStream::new(rx))
}
