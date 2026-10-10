/* The clock example from C: the host implements the clock facility. Run with `lawless`, the
   host's clock does not tick, breaking the assumption the claims rest on: the program still
   runs, and computes what the claims do not promise. Prints what it found.
   TEST0322: deadlines on a host's clock from C, and on a clock breaking the assumption. */
#include "timing.h"

#include <stdio.h>
#include <string.h>

static uint64_t rate = 1000;
static uint64_t now_ticks = 0;

static int32_t ticks_per_second(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result,
                                lungo_error **error) {
    (void)ctx, (void)args, (void)n, (void)error;
    *result = lungo_value_nat(rate);
    return LUNGO_OK;
}

static int32_t now(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    (void)ctx, (void)args, (void)n, (void)error;
    *result = lungo_value_nat(now_ticks);
    return LUNGO_OK;
}

/* f(x) for a function of one Nat to a Nat that must succeed. */
static uint64_t call(int32_t (*f)(const lungo_value *, lungo_value **, lungo_error **), uint64_t x) {
    lungo_value *in = lungo_value_nat(x), *out = NULL;
    lungo_error *error = NULL;
    uint64_t y = UINT64_MAX;
    if (f(in, &out, &error) != LUNGO_OK || !lungo_value_get_nat(out, &y)) {
        fprintf(stderr, "a call failed\n");
    }
    lungo_value_free(out);
    lungo_value_free(in);
    return y;
}

int main(int argc, char **argv) {
    int lawless = argc == 2 && strcmp(argv[1], "lawless") == 0;
    if (lawless) rate = 0;
    timing_implement_clock_ticks_per_second(ticks_per_second, NULL, NULL);
    timing_implement_clock_now(now, NULL, NULL);
    uint64_t round_trip = call(timing_to_seconds, call(timing_to_ticks, 7));
    uint64_t deadline = call(timing_deadline_in, 5);
    now_ticks += 2500;
    uint64_t left = call(timing_remaining, deadline);
    printf("round trip of 7 s: %llu s; deadline in 5 s, after 2.5 s: %llu s left\n", (unsigned long long)round_trip,
           (unsigned long long)left);
    /* What the package says the claim rests on. */
    printf("assumes Timing.Ticks: %s\n", strstr(timing_assurance_json(), "\"Timing.Ticks\"") ? "yes" : "no");
    return 0;
}
