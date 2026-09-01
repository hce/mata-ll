local m = {}
for i = 2000, 1, -1 do
    m[i] = i * i
end
local acc = 0
for i = 1000000, 1, -1 do
    local v = m[i % 2000 + 1]
    if v then
        acc = (acc + v) % 1000000007
    end
end
print(acc)
