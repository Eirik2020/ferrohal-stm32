/* STM32F405RG. CCM RAM stays out of it: DMA can't reach it. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
  /* The top 32 bytes hold the stop record (src/ident.rs, STOP_RECORD), which
     no section and no stack may reach. */
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K - 32
}
