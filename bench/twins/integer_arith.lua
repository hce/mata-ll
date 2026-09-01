local acc = 0
for i = 1, 200000 do
    acc = (acc * 7 + i) % 1000000007
end
print(acc)
