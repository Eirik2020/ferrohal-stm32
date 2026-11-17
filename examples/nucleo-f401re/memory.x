/* STM32F401RE: 512 KiB flash, 96 KiB SRAM (DS10086). stm32-metapac has no
   memory.x of its own. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 512K
  RAM   : ORIGIN = 0x20000000, LENGTH = 96K
}
