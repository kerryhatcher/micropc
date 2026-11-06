/* Memory layout for the RP2040 (2 MiB external QSPI flash, 256 KiB SRAM).
 * The first 256 bytes of flash hold the second-stage bootloader (BOOT2). */
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM   : ORIGIN = 0x20000000, LENGTH = 256K
}

EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    /* Second-stage bootloader, required by the RP2040 boot ROM. */
    .boot2 ORIGIN(BOOT2) :
    {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;
