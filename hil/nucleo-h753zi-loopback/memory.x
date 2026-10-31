/* STM32H753ZI. Everything runs from AXI SRAM: DMA1 and DMA2 cannot reach
   the DTCM at 0x2000_0000. The top 16 KiB of AXI SRAM hold the DMA
   buffers; `init` makes that block non-cacheable with MPU region 0 before
   it turns the data cache on. An MPU region's base must be aligned to its
   size, which 0x2407_C000 is for 16 KiB. */
MEMORY
{
  FLASH  : ORIGIN = 0x08000000, LENGTH = 2048K
  RAM    : ORIGIN = 0x24000000, LENGTH = 496K
  DMA_NC : ORIGIN = 0x2407C000, LENGTH = 16K
}

SECTIONS
{
  /* Not loaded or zeroed: `init` clears the buffers itself. */
  .dma_buffers (NOLOAD) : ALIGN(32)
  {
    *(.dma_buffers .dma_buffers.*);
  } > DMA_NC
}
/* Before .data, so cortex-m-rt still finds the end of RAM use after .uninit. */
INSERT BEFORE .data;
