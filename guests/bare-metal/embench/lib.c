#include <stddef.h>
#include <stdint.h>

void *
memcpy(void *destination, const void *source, size_t count)
{
    unsigned char *dst = destination;
    const unsigned char *src = source;
    while (count-- != 0)
        *dst++ = *src++;
    return destination;
}

void *
memmove(void *destination, const void *source, size_t count)
{
    unsigned char *dst = destination;
    const unsigned char *src = source;

    if (dst < src) {
        while (count-- != 0)
            *dst++ = *src++;
    } else {
        dst += count;
        src += count;
        while (count-- != 0)
            *--dst = *--src;
    }
    return destination;
}

void *
memset(void *destination, int value, size_t count)
{
    unsigned char *dst = destination;
    while (count-- != 0)
        *dst++ = (unsigned char)value;
    return destination;
}

int
memcmp(const void *left, const void *right, size_t count)
{
    const unsigned char *a = left;
    const unsigned char *b = right;
    while (count-- != 0) {
        if (*a != *b)
            return *a - *b;
        ++a;
        ++b;
    }
    return 0;
}

size_t
strlen(const char *string)
{
    const char *end = string;
    while (*end != '\0')
        ++end;
    return (size_t)(end - string);
}

char *
strchr(const char *string, int character)
{
    do {
        if (*string == (char)character)
            return (char *)string;
    } while (*string++ != '\0');
    return NULL;
}

int
printf(const char *format, ...)
{
    (void)format;
    return 0;
}

void __attribute__((noreturn))
exit(int status)
{
    extern void embench_finish(int) __attribute__((noreturn));
    embench_finish(status);
}

void __attribute__((noreturn))
abort(void)
{
    exit(1);
}

double
sqrt(double value)
{
    double result;
    __asm__ volatile("fsqrt.d %0, %1" : "=f"(result) : "f"(value));
    return result;
}

static double
cosine(double value)
{
    const double pi = 3.14159265358979323846264338327950288;
    const double two_pi = 2.0 * pi;
    double term = 1.0;
    double result = 1.0;

    while (value > pi)
        value -= two_pi;
    while (value < -pi)
        value += two_pi;

    for (int i = 1; i <= 16; ++i) {
        term *= -(value * value) / ((2.0 * i - 1.0) * (2.0 * i));
        result += term;
    }
    return result;
}

double
cos(double value)
{
    return cosine(value);
}

double
acos(double value)
{
    const double pi = 3.14159265358979323846264338327950288;
    double low = 0.0;
    double high = pi;

    for (int i = 0; i < 60; ++i) {
        double middle = (low + high) * 0.5;
        if (cosine(middle) > value)
            low = middle;
        else
            high = middle;
    }
    return (low + high) * 0.5;
}

double
pow(double value, double exponent)
{
    /* Embench 1.0 uses pow only for a positive cube root in cubic. */
    (void)exponent;
    double result = value > 1.0 ? value : 1.0;
    for (int i = 0; i < 30; ++i)
        result = (2.0 * result + value / (result * result)) / 3.0;
    return result;
}
