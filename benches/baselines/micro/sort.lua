-- Sort 200k LCG-generated ints with the stdlib sort, checksum the result.
local xs = {}
local x = 42
for i = 1, 200000 do
  x = (x * 48271) % 2147483647
  xs[i] = x % 100000
end
local t0 = os.clock()
table.sort(xs)
local t1 = os.clock()
local chk = xs[1] + xs[100000] + xs[200000]
print(string.format("RESULT %d MS %d", chk, math.floor((t1 - t0) * 1000)))
