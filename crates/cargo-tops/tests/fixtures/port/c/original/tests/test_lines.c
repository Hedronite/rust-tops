#include <assert.h>
#include <stddef.h>

size_t count_lines(const char *s);

static void test_no_lines(void) {
    assert(count_lines("") == 0);
}

static void test_trailing_newline(void) {
    assert(count_lines("a\nb\n") == 2);
}

TEST(lines, unterminated_last_line) {
    assert(count_lines("a\nb") == 2);
}
