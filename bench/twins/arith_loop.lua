local acc = 0
for i = 1, 5000000 do
    acc = (acc + i) % 1000000007
end
print(acc)
