-- String building: 60k appends (naive concat, same as the others)
local t0 = os.clock()
local s = ""
local i = 0
while i < 60000 do
  s = s .. "ab" .. tostring(i % 10)
  i = i + 1
end
local t1 = os.clock()
print(string.format("RESULT %d MS %d", #s, math.floor((t1 - t0) * 1000)))
