#include <linux/perf_event.h>
#include <stdint.h>
#include <stdio.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <unistd.h>

static int cycle_fd = -1;
static int instruction_fd = -1;

static int open_counter(uint64_t config, int group_fd)
{
  struct perf_event_attr attr;
  memset(&attr, 0, sizeof(attr));
  attr.type = PERF_TYPE_HARDWARE;
  attr.size = sizeof(attr);
  attr.config = config;
  attr.disabled = 1;

  return syscall(SYS_perf_event_open, &attr, 0, -1, group_fd, 0);
}

void setStats(int enable)
{
  uint64_t cycles;
  uint64_t instructions;

  if (enable) {
    cycle_fd = open_counter(PERF_COUNT_HW_CPU_CYCLES, -1);
    instruction_fd = open_counter(PERF_COUNT_HW_INSTRUCTIONS, cycle_fd);
    if (cycle_fd < 0 || instruction_fd < 0) {
      perror("perf_event_open");
      exit(2);
    }

    ioctl(cycle_fd, PERF_EVENT_IOC_RESET, PERF_IOC_FLAG_GROUP);
    ioctl(cycle_fd, PERF_EVENT_IOC_ENABLE, PERF_IOC_FLAG_GROUP);
    return;
  }

  ioctl(cycle_fd, PERF_EVENT_IOC_DISABLE, PERF_IOC_FLAG_GROUP);
  if (read(cycle_fd, &cycles, sizeof(cycles)) != sizeof(cycles) ||
      read(instruction_fd, &instructions, sizeof(instructions)) !=
          sizeof(instructions)) {
    perror("read perf counter");
    exit(2);
  }

  close(instruction_fd);
  close(cycle_fd);
  instruction_fd = -1;
  cycle_fd = -1;

  printf("cycle = %llu\n", (unsigned long long)cycles);
  printf("instret = %llu\n", (unsigned long long)instructions);
}

void *riscv_test_memcpy(void *dest, const void *src, size_t len)
{
  if ((((uintptr_t)dest | (uintptr_t)src | len) & (sizeof(uintptr_t) - 1)) == 0) {
    const uintptr_t *s = src;
    uintptr_t *d = dest;
    uintptr_t *end = (uintptr_t *)((char *)dest + len);
    while (d + 8 < end) {
      uintptr_t reg[8] = {s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]};
      d[0] = reg[0]; d[1] = reg[1]; d[2] = reg[2]; d[3] = reg[3];
      d[4] = reg[4]; d[5] = reg[5]; d[6] = reg[6]; d[7] = reg[7];
      d += 8;
      s += 8;
    }
    while (d < end)
      *d++ = *s++;
  } else {
    const char *s = src;
    char *d = dest;
    while (d < (char *)dest + len)
      *d++ = *s++;
  }
  return dest;
}
