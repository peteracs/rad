// Sort 200k LCG-generated ints with the stdlib sort, checksum the result.
// MINSTD: max intermediate 2^31 * 48271 < 2^53, exact in doubles.
const xs = [];
let x = 42;
for (let i = 0; i < 200000; i++) {
    x = (x * 48271) % 2147483647;
    xs.push(x % 100000);
}
const t0 = Date.now();
xs.sort((a, b) => a - b);
const t1 = Date.now();
const chk = xs[0] + xs[99999] + xs[199999];
print_line(`RESULT ${chk} MS ${t1 - t0}`);
