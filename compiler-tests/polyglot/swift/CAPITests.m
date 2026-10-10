// The generated package's C API from Objective-C.
@import PolyglotProgram;
@import XCTest;

@interface CAPITests : XCTestCase
@end

static int32_t scale(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    (void)ctx;
    (void)n;
    (void)error;
    uint64_t x = 0;
    lungo_value_get_nat(args[0], &x);
    *result = lungo_value_nat(x * 10);
    return LUNGO_OK;
}

static int32_t record(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    (void)ctx;
    (void)n;
    (void)args;
    (void)error;
    *result = lungo_value_unit();
    return LUNGO_OK;
}

@implementation CAPITests

+ (void)setUp {
    polyglot_implement_scaler_host_scale(scale, NULL, NULL);
    polyglot_implement_journal_host_record(record, NULL, NULL);
}

// TEST0059: host Facilities Through The CAPI
- (void)test0059_HostFacilitiesThroughTheCAPI {
    lungo_value *items[2] = {lungo_value_nat(1), lungo_value_nat(2)};
    lungo_value *list = lungo_value_list(items, 2);
    lungo_value *result = NULL;
    lungo_error *error = NULL;
    XCTAssertEqual(polyglot_scaled_sum(list, &result, &error), LUNGO_OK);
    uint64_t sum = 0;
    XCTAssertTrue(lungo_value_get_nat(result, &sum));
    XCTAssertEqual(sum, 30u);
    lungo_value_free(result);
    lungo_value_free(list);
}

// TEST0060: factorial Through The CAPI
- (void)test0060_FactorialThroughTheCAPI {
    lungo_value *n = lungo_value_nat(20);
    lungo_value *result = NULL;
    lungo_error *error = NULL;
    XCTAssertEqual(polyglot_factorial(n, &result, &error), LUNGO_OK);
    char *digits = lungo_value_number_string(result);
    XCTAssertEqualObjects(@(digits), @"2432902008176640000");
    lungo_string_free(digits);
    lungo_value_free(result);
    lungo_value_free(n);
}

// TEST0061: constructors And Errors
- (void)test0061_ConstructorsAndErrors {
    lungo_value *shape = polyglot_shape_empty();
    lungo_value *result = NULL;
    lungo_error *error = NULL;
    XCTAssertEqual(polyglot_area(shape, &result, &error), LUNGO_OK);
    XCTAssertEqual(lungo_value_get_float(result), 0.0);
    lungo_value_free(result);
    lungo_value *x = lungo_value_char('x');
    XCTAssertEqual(polyglot_parse_digit(x, &result, &error), LUNGO_FAILED);
    XCTAssertEqual(lungo_error_kind(error), LUNGO_ERROR_VALUE);
    lungo_error_free(error);
    lungo_value_free(x);
    lungo_value_free(shape);
}

@end
