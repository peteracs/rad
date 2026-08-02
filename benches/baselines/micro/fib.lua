-- fib(30), recursive
local function fib(n)
  if n < 2 then return n end
  return fib(n - 1) + fib(n - 2)
end

local t0 = os.clock()
local r = fib(30)
local t1 = os.clock()
print(string.format("RESULT %d MS %d", r, math.floor((t1 - t0) * 1000)))
