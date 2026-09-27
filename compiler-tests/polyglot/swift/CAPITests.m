// The generated package's C API from Objective-C.
@import PolyglotProgram;
@import XCTest;

@interface CAPITests : XCTestCase
@end

@implementation CAPITests

- (void)testFactorialThroughTheCAPI {
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

- (void)testConstructorsAndErrors {
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
