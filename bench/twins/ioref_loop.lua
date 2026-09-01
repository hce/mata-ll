local acc = 0
for i = 1000000, 1, -1 do
    acc = (acc + i) % 1000000007
end
print(acc)
