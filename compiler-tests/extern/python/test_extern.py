"""A value made by one program is used by the other: both run on the one runtime."""

import provider
from host import consumer

p = provider.mk_pos(3)
assert p is not None
d = consumer.double(p)
assert provider.value(d) == 6, provider.value(d)
q = provider.make_pair(1, "apples")
assert consumer.describe(consumer.count(d, q)) == "apples: 7"
print("ok")
