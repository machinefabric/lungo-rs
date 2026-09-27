/* The polyglot assertions, in C, on the generated C API. Prints "ok" when every one holds. */
#include "polyglot.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define CHECK(cond)                                                                   \
    do {                                                                              \
        if (!(cond)) {                                                                \
            fprintf(stderr, "%s:%d: check failed: %s\n", __FILE__, __LINE__, #cond); \
            failures++;                                                               \
        }                                                                             \
    } while (0)

/* The outputs of the call being checked. */
static lungo_value *result_;
static lungo_error *error_;

/* The result of a call that must succeed. */
static lungo_value *ok_(int32_t status, const char *call, int line) {
    if (status != LUNGO_OK) {
        fprintf(stderr, "test.c:%d: %s failed (%d): %s\n", line, call, status,
                error_ ? lungo_error_message(error_) : "no error");
        exit(1);
    }
    return result_;
}

#define OK(call) ok_((call), #call, __LINE__)

static int string_is(const lungo_value *v, const char *expected) {
    size_t len;
    const char *s = lungo_value_get_string(v, &len);
    return len == strlen(expected) && memcmp(s, expected, len) == 0;
}

static uint64_t nat(const lungo_value *v) {
    uint64_t x = 0;
    CHECK(lungo_value_get_nat(v, &x));
    return x;
}

/* Host externs. */
static char log_lines[8][64];
static size_t log_count = 0;

static int32_t scale(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    (void)error;
    CHECK(n == 1);
    *result = lungo_value_nat(nat(args[0]) * *(uint64_t *)ctx);
    return LUNGO_OK;
}

static int32_t record(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    (void)ctx;
    CHECK(n == 1);
    if (string_is(args[0], "fail")) {
        *error = lungo_error_io("the host refuses to record `fail`");
        return LUNGO_FAILED;
    }
    size_t len;
    const char *s = lungo_value_get_string(args[0], &len);
    snprintf(log_lines[log_count++], sizeof log_lines[0], "%.*s", (int)len, s);
    *result = lungo_value_unit();
    return LUNGO_OK;
}

/* A host function passed to Lean: x ↦ 2x + 1. */
static int32_t twice_plus_one(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result,
                              lungo_error **error) {
    (void)error;
    (*(int *)ctx)++;
    CHECK(n == 1);
    *result = lungo_value_nat(2 * nat(args[0]) + 1);
    return LUNGO_OK;
}

static int dropped = 0;
static void drop_counter(void *ctx) {
    (void)ctx;
    dropped++;
}

static lungo_value *point(double x, double y, const char *label, uint8_t tag) {
    return polyglot_point_mk(lungo_value_float(x), lungo_value_float(y), lungo_value_cstring(label),
                             lungo_value_uint8(tag));
}

int main(void) {
    static uint64_t factor = 10;
    polyglot_implement_host_scale(scale, &factor, NULL);
    polyglot_implement_host_record(record, NULL, NULL);

    /* Nat beyond 64 bits. */
    lungo_value *n = lungo_value_nat(25);
    lungo_value *f = OK(polyglot_factorial(n, &result_, &error_));
    char *digits = lungo_value_number_string(f);
    CHECK(strcmp(digits, "15511210043330985984000000") == 0);
    lungo_string_free(digits);
    lungo_value_free(f);
    lungo_value_free(n);

    /* Int. */
    lungo_value *big = lungo_value_int_parse("-123456789012345678901234567890");
    lungo_value *neg = OK(polyglot_negate(big, &result_, &error_));
    digits = lungo_value_number_string(neg);
    CHECK(strcmp(digits, "123456789012345678901234567890") == 0);
    lungo_string_free(digits);
    lungo_value_free(neg);
    lungo_value_free(big);

    /* Structures and inductives. */
    lungo_value *p = point(1.5, -2.0, "p", 7);
    lungo_value *dx = lungo_value_float(1.0), *dy = lungo_value_float(0.5);
    lungo_value *moved = OK(polyglot_move_by(p, dx, dy, &result_, &error_));
    CHECK(lungo_value_get_float(polyglot_point_x(moved)) == 2.5);
    CHECK(lungo_value_get_float(polyglot_point_y(moved)) == -1.5);
    CHECK(string_is(polyglot_point_label(moved), "p"));
    CHECK(lungo_value_get_uint8(polyglot_point_tag(moved)) == 7);
    lungo_value *text = OK(polyglot_describe(moved, &result_, &error_));
    CHECK(string_is(text, "p#7 at (2.500000, -1.500000)"));
    lungo_value_free(text);
    lungo_value *rect = polyglot_shape_rect(lungo_value_clone(moved), lungo_value_float(3.0), lungo_value_float(4.0));
    lungo_value *area = OK(polyglot_area(rect, &result_, &error_));
    CHECK(lungo_value_get_float(area) == 12.0);
    CHECK(lungo_value_ctor_index(rect) == POLYGLOT_SHAPE_RECT);
    lungo_value_free(area);
    lungo_value *empty = polyglot_shape_empty();
    area = OK(polyglot_area(empty, &result_, &error_));
    CHECK(lungo_value_get_float(area) == 0.0);
    lungo_value_free(area);
    lungo_value_free(empty);
    lungo_value_free(rect);
    lungo_value_free(moved);
    lungo_value_free(dx);
    lungo_value_free(dy);
    lungo_value_free(p);

    /* Scalars, characters, bytes, floats, options, pairs, arrays, Except. */
    lungo_value *a = lungo_value_uint64(UINT64_MAX), *b = lungo_value_int32(-5), *c = lungo_value_char(0x3bb),
                *d = lungo_value_bool(true);
    lungo_value *mixed = OK(polyglot_mix(a, b, c, d, &result_, &error_));
    CHECK(string_is(mixed, "18446744073709551615/-5/\xce\xbb/true"));
    lungo_value_free(mixed);
    lungo_value_free(a);
    lungo_value_free(b);
    lungo_value_free(c);
    lungo_value_free(d);
    const uint8_t bytes[] = {1, 2, 3};
    lungo_value *ba = lungo_value_byte_array(bytes, 3);
    lungo_value *rev = OK(polyglot_reverse_bytes(ba, &result_, &error_));
    size_t len;
    const uint8_t *rb = lungo_value_get_bytes(rev, &len);
    CHECK(len == 3 && rb[0] == 3 && rb[2] == 1);
    lungo_value_free(rev);
    lungo_value_free(ba);
    const double floats[] = {0.5, 0.25, 2.0};
    lungo_value *fa = lungo_value_float_array(floats, 3);
    lungo_value *sum = OK(polyglot_sum_floats(fa, &result_, &error_));
    CHECK(lungo_value_get_float(sum) == 2.75);
    lungo_value_free(sum);
    lungo_value_free(fa);
    lungo_value *words = lungo_value_cstring("hello world");
    lungo_value *first = OK(polyglot_first_word(words, &result_, &error_));
    CHECK(lungo_value_get_option(first) && string_is(lungo_value_get_option(first), "hello"));
    lungo_value_free(first);
    lungo_value_free(words);
    lungo_value *blank = lungo_value_cstring("");
    first = OK(polyglot_first_word(blank, &result_, &error_));
    CHECK(lungo_value_get_option(first) == NULL);
    lungo_value_free(first);
    lungo_value_free(blank);
    lungo_value *pair = lungo_value_prod(lungo_value_cstring("x"), lungo_value_nat(9));
    lungo_value *swapped = OK(polyglot_swap(pair, &result_, &error_));
    CHECK(nat(lungo_value_first(swapped)) == 9 && string_is(lungo_value_second(swapped), "x"));
    lungo_value_free(swapped);
    lungo_value_free(pair);
    lungo_value *ten = lungo_value_nat(10);
    lungo_value *evens = OK(polyglot_evens(ten, &result_, &error_));
    CHECK(lungo_value_kind(evens) == LUNGO_ARRAY && lungo_value_count(evens) == 5);
    CHECK(nat(lungo_value_item(evens, 4)) == 8);
    lungo_value_free(evens);
    lungo_value *zero = lungo_value_nat(0);
    lungo_value *quotient = OK(polyglot_divide(ten, zero, &result_, &error_));
    CHECK(!lungo_value_is_ok(quotient) && string_is(lungo_value_get_except(quotient), "division by zero"));
    lungo_value_free(quotient);

    /* IO and EIO errors. */
    lungo_value *result = NULL;
    lungo_error *error = NULL;
    CHECK(polyglot_checked_div(ten, zero, &result, &error) == LUNGO_FAILED);
    CHECK(result == NULL && lungo_error_kind(error) == LUNGO_ERROR_IO);
    CHECK(strcmp(lungo_error_message(error), "checkedDiv: division by zero") == 0);
    lungo_error_free(error);
    lungo_value *x = lungo_value_char('x');
    CHECK(polyglot_parse_digit(x, &result, &error) == LUNGO_FAILED);
    CHECK(lungo_error_kind(error) == LUNGO_ERROR_VALUE && string_is(lungo_error_value(error), "not a digit: x"));
    lungo_error_free(error);
    lungo_value_free(x);
    lungo_value *seven = lungo_value_char('7');
    lungo_value *digit = OK(polyglot_parse_digit(seven, &result_, &error_));
    CHECK(nat(digit) == 7);
    lungo_value_free(digit);
    lungo_value_free(seven);

    /* Arguments that do not match are rejected, not misread. */
    lungo_value *wrong = lungo_value_cstring("ten");
    CHECK(polyglot_factorial(wrong, &result, &error) == LUNGO_MALFORMED);
    CHECK(lungo_error_kind(error) == LUNGO_ERROR_MALFORMED);
    lungo_error_free(error);
    lungo_value_free(wrong);

    /* Polymorphic functions take their type arguments. */
    lungo_type *nat_type = lungo_type_simple(LUNGO_NAT);
    lungo_value *items[3] = {lungo_value_nat(1), lungo_value_nat(2), lungo_value_nat(3)};
    lungo_value *list = lungo_value_list(items, 3);
    lungo_value *tree = OK(polyglot_of_list(nat_type, list, &result_, &error_));
    lungo_value *size = OK(polyglot_size(nat_type, tree, &result_, &error_));
    CHECK(nat(size) == 3);
    lungo_value *mirrored = OK(polyglot_mirror(nat_type, tree, &result_, &error_));
    lungo_value *back = OK(polyglot_to_list(nat_type, mirrored, &result_, &error_));
    CHECK(lungo_value_count(back) == 3 && nat(lungo_value_item(back, 0)) == 3);
    CHECK(lungo_value_ctor_index(mirrored) == POLYGLOT_TREE_NODE);
    lungo_value_free(back);
    lungo_value_free(mirrored);
    lungo_value_free(size);
    lungo_value_free(tree);
    lungo_value_free(list);
    lungo_type *string_type = lungo_type_simple(LUNGO_STRING);
    lungo_value *stree = polyglot_tree_node(polyglot_tree_leaf(), lungo_value_cstring("s"), polyglot_tree_leaf());
    size = OK(polyglot_size(string_type, stree, &result_, &error_));
    CHECK(nat(size) == 1);
    lungo_value_free(size);
    CHECK(polyglot_size(nat_type, stree, &result, &error) == LUNGO_MALFORMED);
    lungo_error_free(error);
    lungo_value_free(stree);
    lungo_type_free(string_type);

    /* Functions in both directions. */
    const lungo_type *params[1] = {nat_type};
    lungo_type *fn_type = lungo_type_function(params, 1, nat_type);
    int calls = 0;
    lungo_value *host_fn = lungo_value_function(fn_type, twice_plus_one, &calls, drop_counter);
    lungo_value *three = lungo_value_nat(3);
    lungo_value *twice = OK(polyglot_apply_twice(host_fn, three, &result_, &error_));
    CHECK(nat(twice) == 15 && calls == 2);
    lungo_value_free(twice);
    lungo_value_free(host_fn);
    CHECK(dropped == 1);
    lungo_value *five = lungo_value_nat(5);
    lungo_value *add_five = OK(polyglot_adder(five, three, &result_, &error_));
    CHECK(nat(add_five) == 8);
    lungo_value_free(add_five);
    lungo_value_free(five);
    lungo_type_free(fn_type);
    lungo_value_free(three);

    /* Opaque values by handle. */
    lungo_value *counter = OK(polyglot_new_counter(ten, &result_, &error_));
    CHECK(lungo_value_kind(counter) == LUNGO_OPAQUE);
    lungo_value *v1 = OK(polyglot_bump(counter, &result_, &error_));
    lungo_value *copy = lungo_value_clone(counter);
    lungo_value_free(counter);
    lungo_value *v2 = OK(polyglot_bump(copy, &result_, &error_));
    CHECK(nat(v1) == 11 && nat(v2) == 12);
    lungo_value_free(v1);
    lungo_value_free(v2);
    lungo_value_free(copy);

    /* Host externs. */
    lungo_value *nums[3] = {lungo_value_nat(1), lungo_value_nat(2), lungo_value_nat(3)};
    lungo_value *numbers = lungo_value_list(nums, 3);
    lungo_value *scaled = OK(polyglot_scaled_sum(numbers, &result_, &error_));
    CHECK(nat(scaled) == 60);
    lungo_value_free(scaled);
    lungo_value_free(numbers);
    lungo_value *lines_ok[2] = {lungo_value_cstring("a"), lungo_value_cstring("b")};
    lungo_value *lines = lungo_value_list(lines_ok, 2);
    lungo_value *count = OK(polyglot_record_all(lines, &result_, &error_));
    CHECK(nat(count) == 2 && log_count == 2 && strcmp(log_lines[1], "b") == 0);
    lungo_value_free(count);
    lungo_value_free(lines);
    lungo_value *lines_fail[3] = {lungo_value_cstring("c"), lungo_value_cstring("fail"), lungo_value_cstring("d")};
    lines = lungo_value_list(lines_fail, 3);
    CHECK(polyglot_record_all(lines, &result, &error) == LUNGO_FAILED);
    CHECK(strcmp(lungo_error_message(error), "the host refuses to record `fail`") == 0);
    CHECK(log_count == 3);
    lungo_error_free(error);
    lungo_value_free(lines);

    lungo_value_free(ten);
    lungo_value_free(zero);
    lungo_type_free(nat_type);

    /* main. */
    const char *argv[2] = {"one", "two"};
    CHECK(polyglot_run_main(2, argv) == 2);

    if (failures) {
        fprintf(stderr, "%d checks failed\n", failures);
        return 1;
    }
    printf("ok\n");
    return 0;
}
