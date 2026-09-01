local acc = 0
for i = 20000, 1, -1 do
    local s = '{"name":"person-' .. i .. '","age":' .. i .. ',"tags":["t","x' .. i .. '"]}'
    acc = acc + #s
end
print(acc)
