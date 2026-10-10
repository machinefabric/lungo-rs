/* The codec example from C: the same proved codec, and its claims. Prints "ok" when every check
   holds. TEST0318: the proved codec from C. */
#include "varint.h"

#include <stdio.h>
#include <string.h>

static int failures = 0;

#define CHECK(cond)                                                                   \
    do {                                                                              \
        if (!(cond)) {                                                                \
            fprintf(stderr, "%s:%d: check failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                               \
        }                                                                             \
    } while (0)

static lungo_value *bytes(const uint8_t *xs, size_t n) {
    lungo_value *items[16];
    for (size_t i = 0; i < n; i++) items[i] = lungo_value_uint8(xs[i]);
    return lungo_value_list(items, n);
}

static int is_bytes(const lungo_value *v, const uint8_t *xs, size_t n) {
    if (lungo_value_count(v) != n) return 0;
    for (size_t i = 0; i < n; i++)
        if (lungo_value_get_uint8(lungo_value_item(v, i)) != xs[i]) return 0;
    return 1;
}

int main(void) {
    lungo_value *result = NULL;
    lungo_error *error = NULL;

    lungo_value *n = lungo_value_nat(624485);
    CHECK(varint_encode(n, &result, &error) == LUNGO_OK);
    const uint8_t leb[] = {0xe5, 0x8e, 0x26};
    CHECK(is_bytes(result, leb, 3));
    lungo_value_free(result);
    lungo_value_free(n);

    const uint8_t input[] = {0xac, 0x02, 0x09};
    lungo_value *in = bytes(input, 3);
    CHECK(varint_decode(in, &result, &error) == LUNGO_OK);
    const lungo_value *pair = lungo_value_get_option(result);
    uint64_t x = 0;
    CHECK(pair && lungo_value_get_nat(lungo_value_first(pair), &x) && x == 300);
    const uint8_t rest[] = {0x09};
    CHECK(pair && is_bytes(lungo_value_second(pair), rest, 1));
    lungo_value_free(result);
    lungo_value_free(in);

    const uint8_t truncated[] = {0x80, 0x81};
    in = bytes(truncated, 2);
    CHECK(varint_decode(in, &result, &error) == LUNGO_OK);
    CHECK(lungo_value_get_option(result) == NULL);
    lungo_value_free(result);
    lungo_value_free(in);

    CHECK(strstr(varint_assurance_json(), "\"name\": \"Varint.decode_encode\"") != NULL);

    if (failures) {
        fprintf(stderr, "%d checks failed\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
