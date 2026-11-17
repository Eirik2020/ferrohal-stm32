/* STM32F767ZI: 2 MiB flash; DTCM (128 KiB) then SRAM1 (368 KiB) and SRAM2
   (16 KiB), contiguous from 0x2000_0000 (RM0410 section 2.3). The DMA reaches
   all of it, the DTCM through the core's AHBS port; the bench runs leave the
   data cache off. stm32-metapac has no memory.x of its own. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 2048K
  RAM   : ORIGIN = 0x20000000, LENGTH = 512K
}

/* The bench identity block (hil/common/ident.rs): 256 bytes at a fixed
   address after the vector table, read by hil/rig_status.py over the debug
   probe. The F767's table (126 entries) ends at 0x1F8. */
_stext = ORIGIN(FLASH) + 0x500;
SECTIONS
{
  .ferro_ident ORIGIN(FLASH) + 0x400 :
  {
    KEEP(*(.ferro_ident));
  } > FLASH
}
INSERT AFTER .vector_table;
