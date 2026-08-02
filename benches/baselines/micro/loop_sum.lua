-- 10M-iteration arithmetic loop
local t0 = os.clock()
local acc = 0
local i = 0
while i < 10000000 do
  acc = acc + i % 7
  i = i + 1
end
local t1 = os.clock()
print(string.format("RESULT %d MS %d", acc, math.floor((t1 - t0) * 1000)))
