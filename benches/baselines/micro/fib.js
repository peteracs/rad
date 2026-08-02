// fib(30), recursive
function fib(n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

const t0 = Date.now();
const r = fib(30);
const t1 = Date.now();
print_line(`RESULT ${r} MS ${t1 - t0}`);
