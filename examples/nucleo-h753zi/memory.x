/* STM32H753ZI: 2 MiB flash; RAM is the 512 KiB AXI SRAM (RM0433, memory
   map). The DMA controllers cannot reach the DTCM at 0x2000_0000, so a
   firmware using DMA keeps its buffers here too. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 2048K
  RAM   : ORIGIN = 0x24000000, LENGTH = 512K
}
