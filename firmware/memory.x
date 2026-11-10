/* STM32F401CC (Black Pill): 256K flash, 64K RAM. F401CE (512K/96K) works with these values too. */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 256K
  RAM   : ORIGIN = 0x20000000, LENGTH = 64K
}
