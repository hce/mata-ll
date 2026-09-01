local parts = {}
for i = 1, 20000 do
    parts[#parts + 1] = i .. ","
end
print(#table.concat(parts))
