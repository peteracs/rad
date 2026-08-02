// 10M-iteration arithmetic loop
const t0 = Date.now();
let acc = 0;
let i = 0;
while (i < 10000000) {
    acc = acc + i % 7;
    i = i + 1;
}
const t1 = Date.now();
print_line(`RESULT ${acc} MS ${t1 - t0}`);
