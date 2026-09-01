local m = {}
local acc = 0
for i = 30000, 1, -1 do
    m[i % 500] = i * i
    m[(i * 7) % 500] = nil
    local v = m[(i * 3) % 500]
    if v then
        acc = (acc + v) % 1000000007
    end
end
local n = 0
for _ in pairs(m) do
    n = n + 1
end
print((acc + n) % 1000000007)
