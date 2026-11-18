#include <stddef.h>

size_t count_lines(const char *s) {
    size_t n = 0;
    int pending = 0;
    for (; *s; s++) {
        pending = 1;
        if (*s == '\n') {
            n++;
            pending = 0;
        }
    }
    return n + (size_t)pending;
}
