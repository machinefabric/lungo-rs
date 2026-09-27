/* A value made by one program is used by the other: both run on the one runtime. */
#include "consumer.h"
#include "provider.h"

#include <stdio.h>
#include <string.h>

static int check(int32_t status, lungo_error *error, const char *what) {
    if (status != LUNGO_OK) {
        fprintf(stderr, "%s failed (%d): %s\n", what, status, error ? lungo_error_message(error) : "");
        return 0;
    }
    return 1;
}

int main(void) {
    lungo_value *r = NULL;
    lungo_error *e = NULL;
    lungo_value *three = lungo_value_nat(3);
    lungo_value *p = NULL;
    if (!check(provider_mk_pos(three, &p, &e), e, "mkPos")) return 1;
    const lungo_value *pos = lungo_value_get_option(p);
    if (pos == NULL) return 1;
    lungo_value *d = NULL;
    if (!check(consumer_double(pos, &d, &e), e, "double")) return 1;
    if (!check(provider_value(d, &r, &e), e, "value")) return 1;
    uint64_t n = 0;
    if (!lungo_value_get_nat(r, &n) || n != 6) {
        fprintf(stderr, "value (double 3) = %llu\n", (unsigned long long)n);
        return 1;
    }
    lungo_value_free(r);
    lungo_value *one = lungo_value_nat(1);
    lungo_value *apples = lungo_value_cstring("apples");
    lungo_value *q = NULL, *c = NULL, *s = NULL;
    if (!check(provider_make_pair(one, apples, &q, &e), e, "makePair")) return 1;
    if (!check(consumer_count(d, q, &c, &e), e, "count")) return 1;
    if (!check(consumer_describe(c, &s, &e), e, "describe")) return 1;
    size_t len = 0;
    const char *text = lungo_value_get_string(s, &len);
    if (len != strlen("apples: 7") || memcmp(text, "apples: 7", len) != 0) {
        fprintf(stderr, "describe = %.*s\n", (int)len, text);
        return 1;
    }
    lungo_value_free(s);
    lungo_value_free(c);
    lungo_value_free(q);
    lungo_value_free(apples);
    lungo_value_free(one);
    lungo_value_free(d);
    lungo_value_free(p);
    lungo_value_free(three);
    printf("ok\n");
    return 0;
}
