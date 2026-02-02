// See LICENSE for license details.

#include <stdint.h>
#include <string.h>
#include <stdarg.h>
#include <stdio.h>
#include <limits.h>
#include <sys/signal.h>
#include "util.h"

#define SYS_write 64
#define HTIF_DEV_CONSOLE 1
#define HTIF_CONSOLE_CMD_PUTC 1

#undef strcmp

/* --- SMODE / SBI 3.0 Additions --- */
#ifdef SMODE
struct sbiret {
  long error;
  long value;
};

static inline struct sbiret sbi_ecall(unsigned long arg0, unsigned long arg1,
    unsigned long arg2, unsigned long arg3,
    unsigned long arg4, unsigned long arg5,
    int ext,  int fid)
{
  struct sbiret ret;
  register unsigned long a0 asm("a0") = arg0;
  register unsigned long a1 asm("a1") = arg1;
  register unsigned long a2 asm("a2") = arg2;
  register unsigned long a3 asm("a3") = arg3;
  register unsigned long a4 asm("a4") = arg4;
  register unsigned long a5 asm("a5") = arg5;
  register unsigned long a6 asm("a6") = fid;
  register unsigned long a7 asm("a7") = ext;

  asm volatile ("ecall"
    : "+r"(a0), "+r"(a1)
    : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
    : "memory" );

  ret.error = a0;
  ret.value = a1;
  return ret;
}
#else
volatile uint64_t tohost __attribute__ ((section (".tohost")));
volatile uint64_t fromhost __attribute__ ((section (".tohost")));
#endif

static uintptr_t syscall(uintptr_t which, uint64_t arg0, uint64_t arg1, uint64_t arg2)
{
#ifdef SMODE
  if (which == SYS_write && arg0 == HTIF_DEV_CONSOLE) {
    // SBI 3.0 DBCN_CONSOLE_WRITE_BYTE = extension 0x4442434E, fid 2
    uintptr_t *addr = (uintptr_t*) arg1;
    sbi_ecall(*addr, 0, 0, 0, 0, 0, 0x4442434E, 2);
    return 0;
  }
  return -1;
#else
  volatile uint64_t magic_mem[8] __attribute__((aligned(64)));
  magic_mem[0] = which;
  magic_mem[1] = arg0;
  magic_mem[2] = arg1;
  magic_mem[3] = arg2;
  __sync_synchronize();

  tohost = (uintptr_t)magic_mem;
  while (fromhost == 0)
    ;
  fromhost = 0;

  __sync_synchronize();
  return magic_mem[0];
#endif
}

#define NUM_COUNTERS 2
static uintptr_t counters[NUM_COUNTERS];
static char* counter_names[NUM_COUNTERS];

void setStats(int enable)
{
  int i = 0;
#define READ_CTR(name) do { \
    while (i >= NUM_COUNTERS) ; \
    uintptr_t csr = read_csr(name); \
    if (!enable) { csr -= counters[i]; counter_names[i] = #name; } \
    counters[i++] = csr; \
  } while (0)

#ifdef SMODE
  READ_CTR(cycle);   // S-mode shadow of mcycle
  READ_CTR(instret); // S-mode shadow of minstret
#else
  READ_CTR(mcycle);
  READ_CTR(minstret);
#endif

#undef READ_CTR
}

void __attribute__((noreturn)) tohost_exit(uintptr_t code)
{
#ifdef SMODE
  while (1); // Do not exit in S-mode
#else
  tohost = (code << 1) | 1;
  while (1);
#endif
}

uintptr_t __attribute__((weak)) handle_trap(uintptr_t cause, uintptr_t epc, uintptr_t regs[32])
{
  tohost_exit(1337);
}

void exit(int code)
{
  tohost_exit(code);
}

void abort()
{
  exit(128 + SIGABRT);
}

void printstr(const char* s)
{
  while (*s) {
    syscall(SYS_write, HTIF_DEV_CONSOLE, (uintptr_t)s++, HTIF_CONSOLE_CMD_PUTC);
  }
}

void __attribute__((weak)) thread_entry(int cid, int nc)
{
  while (cid != 0);
}

int __attribute__((weak)) main(int argc, char** argv)
{
  printstr("Implement main(), foo!\n");
  return -1;
}

static void init_tls()
{
  register void* thread_pointer asm("tp");
  extern char _tdata_begin, _tdata_end, _tbss_end;
  size_t tdata_size = &_tdata_end - &_tdata_begin;
  memcpy(thread_pointer, &_tdata_begin, tdata_size);
  size_t tbss_size = &_tbss_end - &_tdata_end;
  memset(thread_pointer + tdata_size, 0, tbss_size);
}

void _init(int cid, int nc)
{
  init_tls();
  thread_entry(cid, nc);

  int ret = main(0, 0);

  char buf[NUM_COUNTERS * 32] __attribute__((aligned(64)));
  char* pbuf = buf;
  for (int i = 0; i < NUM_COUNTERS; i++)
    if (counters[i])
      pbuf += sprintf(pbuf, "%s = %lu\n", counter_names[i], counters[i]);
  if (pbuf != buf)
    printstr(buf);

  exit(ret);
}

#undef putchar
int putchar(int ch)
{
  char c = ch;
  syscall(SYS_write, HTIF_DEV_CONSOLE, (uintptr_t)c, HTIF_CONSOLE_CMD_PUTC);
  return 0;
}

void printhex(uint64_t x)
{
  char str[17];
  int i;
  for (i = 0; i < 16; i++)
  {
    str[15-i] = (x & 0xF) + ((x & 0xF) < 10 ? '0' : 'a'-10);
    x >>= 4;
  }
  str[16] = 0;
  printstr(str);
}

static inline void printnum(void (*putch)(int, void**), void **putdat,
                    unsigned long long num, unsigned base, int width, int padc)
{
  unsigned digs[sizeof(num)*8];
  int pos = 0;
  while (1)
  {
    digs[pos++] = num % base;
    if (num < base) break;
    num /= base;
  }
  while (width-- > pos) putch(padc, putdat);
  while (pos-- > 0) putch(digs[pos] + (digs[pos] >= 10 ? 'a' - 10 : '0'), putdat);
}

static unsigned long long getuint(va_list *ap, int lflag)
{
  if (lflag >= 2) return va_arg(*ap, unsigned long long);
  else if (lflag) return va_arg(*ap, unsigned long);
  else return va_arg(*ap, unsigned int);
}

static long long getint(va_list *ap, int lflag)
{
  if (lflag >= 2) return va_arg(*ap, long long);
  else if (lflag) return va_arg(*ap, long);
  else return va_arg(*ap, int);
}

static void vprintfmt(void (*putch)(int, void**), void **putdat, const char *fmt, va_list ap)
{
  register const char* p;
  const char* last_fmt;
  register int ch;
  unsigned long long num;
  int base, lflag, width, precision;
  char padc;

  while (1) {
    while ((ch = *(unsigned char *) fmt) != '%') {
      if (ch == '\0') return;
      fmt++;
      putch(ch, putdat);
    }
    fmt++;
    last_fmt = fmt;
    padc = ' ';
    width = -1;
    precision = -1;
    lflag = 0;
  reswitch:
    switch (ch = *(unsigned char *) fmt++) {
    case '-': padc = '-'; goto reswitch;
    case '0': padc = '0'; goto reswitch;
    case '1'...'9':
      for (precision = 0; ; ++fmt) {
        precision = precision * 10 + ch - '0';
        ch = *fmt;
        if (ch < '0' || ch > '9') break;
      }
      if (width < 0) width = precision, precision = -1;
      goto reswitch;
    case 'l': lflag++; goto reswitch;
    case 'c': putch(va_arg(ap, int), putdat); break;
    case 's':
      if ((p = va_arg(ap, char *)) == NULL) p = "(null)";
      if (width > 0 && padc != '-')
        for (width -= strnlen(p, precision); width > 0; width--) putch(padc, putdat);
      for (; (ch = *p) != '\0' && (precision < 0 || --precision >= 0); width--) {
        putch(ch, putdat); p++;
      }
      for (; width > 0; width--) putch(' ', putdat);
      break;
    case 'd':
      num = getint(&ap, lflag);
      if ((long long) num < 0) { putch('-', putdat); num = -(long long) num; }
      base = 10; goto signed_number;
    case 'u': base = 10; goto unsigned_number;
    case 'x': base = 16;
    unsigned_number:
      num = getuint(&ap, lflag);
    signed_number:
      printnum(putch, putdat, num, base, width, padc);
      break;
    case '%': putch(ch, putdat); break;
    default: putch('%', putdat); fmt = last_fmt; break;
    }
  }
}

int printf(const char* fmt, ...)
{
  va_list ap;
  va_start(ap, fmt);
  vprintfmt((void*)putchar, 0, fmt, ap);
  va_end(ap);
  return 0;
}

int sprintf(char* str, const char* fmt, ...)
{
  va_list ap;
  char* str0 = str;
  va_start(ap, fmt);
  void sprintf_putch(int ch, void** data) {
    char** pstr = (char**)data;
    **pstr = ch; (*pstr)++;
  }
  vprintfmt(sprintf_putch, (void**)&str, fmt, ap);
  *str = 0;
  va_end(ap);
  return str - str0;
}

void* memcpy(void* dest, const void* src, size_t len)
{
  if ((((uintptr_t)dest | (uintptr_t)src | len) & (sizeof(uintptr_t)-1)) == 0) {
    const uintptr_t* s = src;
    uintptr_t *d = dest;
    uintptr_t *end = (uintptr_t*)((char*)dest + len);
    while (d + 8 < end) {
      uintptr_t reg[8] = {s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]};
      d[0] = reg[0]; d[1] = reg[1]; d[2] = reg[2]; d[3] = reg[3];
      d[4] = reg[4]; d[5] = reg[5]; d[6] = reg[6]; d[7] = reg[7];
      d += 8; s += 8;
    }
    while (d < end) *d++ = *s++;
  } else {
    const char* s = src;
    char *d = dest;
    while (d < (char*)dest + len) *d++ = *s++;
  }
  return dest;
}

void* memset(void* dest, int byte, size_t len)
{
  if ((((uintptr_t)dest | len) & (sizeof(uintptr_t)-1)) == 0) {
    uintptr_t word = byte & 0xFF;
    word |= word << 8; word |= word << 16; word |= word << 16 << 16;
    uintptr_t *d = dest;
    while (d < (uintptr_t*)((char*)dest + len)) *d++ = word;
  } else {
    char *d = dest;
    while (d < (char*)dest + len) *d++ = byte;
  }
  return dest;
}

size_t strlen(const char *s) { const char *p = s; while (*p) p++; return p - s; }
size_t strnlen(const char *s, size_t n) { const char *p = s; while (n-- && *p) p++; return p - s; }

int strcmp(const char* s1, const char* s2)
{
  unsigned char c1, c2;
  do { c1 = *s1++; c2 = *s2++; } while (c1 != 0 && c1 == c2);
  return c1 - c2;
}

char* strcpy(char* dest, const char* src) {
  char* d = dest; while ((*d++ = *src++)); return dest;
}

long atol(const char* str)
{
  long res = 0; int sign = 0;
  while (*str == ' ') str++;
  if (*str == '-' || *str == '+') { sign = *str == '-'; str++; }
  while (*str >= '0' && *str <= '9') { res *= 10; res += *str++ - '0'; }
  return sign ? -res : res;
}
