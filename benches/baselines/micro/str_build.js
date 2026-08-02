// String building: 60k appends (naive concat, same as the others)
const t0 = Date.now();
let s = "";
let i = 0;
while (i < 60000) {
    s = s + "ab" + String(i % 10);
    i = i + 1;
}
const t1 = Date.now();
print_line(`RESULT ${s.length} MS ${t1 - t0}`);
