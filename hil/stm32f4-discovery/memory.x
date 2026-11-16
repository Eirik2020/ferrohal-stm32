/* STM32F407VG: 1 MiB flash; SRAM1 (112 KiB) and SRAM2 (16 KiB), contiguous
   (RM0090 section 2.3). The 64 KiB of CCM RAM at 0x1000_0000 is left out:
   only the core reaches it. stm32-metapac has no memory.x of its own. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}

/* The bench identity block (hil/common/ident.rs): 256 bytes at a fixed
   address after the vector table, read by hil/rig_status.py over the debug
   probe. The largest vector table here (the H753's) ends below 0x300. */
_stext = ORIGIN(FLASH) + 0x500;
SECTIONS
{
  .ferro_ident ORIGIN(FLASH) + 0x400 :
  {
    KEEP(*(.ferro_ident));
  } > FLASH
}
INSERT AFTER .vector_table;
