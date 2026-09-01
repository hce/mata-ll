local acc = 0
for i = 1, 200000 do
    local x = i * 3
    if x % 2 == 1 then
        acc = (acc + x) % 1000000007
    end
end
print(acc)
