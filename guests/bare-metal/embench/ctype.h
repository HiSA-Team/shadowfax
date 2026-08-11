#ifndef SHADOWFAX_EMBENCH_CTYPE_H
#define SHADOWFAX_EMBENCH_CTYPE_H

static inline int isdigit(int c) { return c >= '0' && c <= '9'; }
static inline int isxdigit(int c)
{
    return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}
static inline int isspace(int c)
{
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v';
}
static inline int tolower(int c) { return c >= 'A' && c <= 'Z' ? c - 'A' + 'a' : c; }

#endif
