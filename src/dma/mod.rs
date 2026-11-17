//! DMA, by engine. A die can carry more than one DMA IP at once (the H7 has
//! the stream DMA, a BDMA and an MDMA), so this neutral module holds one
//! engine per kind rather than naming a single IP version:
//!
//! - [`stream`]: the stream DMA (`dma_v1` on the H7, `dma_v2` on the F4).
//!
//! The stream engine's items are also re-exported here, so `dma::Stream`
//! and the rest name it directly. Drivers name their engines the same way
//! (`uart::stream_dma`, `spi::stream_dma`); a channel engine (BDMA) joins
//! as a sibling of `stream`, with its own drivers' engines beside those.
//!
//! Requirements: DMA-001

pub mod stream;
pub use stream::*;
