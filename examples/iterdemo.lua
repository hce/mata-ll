function my_iter()
	return coroutine.wrap(function()
		for i = 1, 10 do
			print("Yielding", i)
			coroutine.yield(i)
		end
	end)
end

local itermll = require "itermll"
print("======================================================================")
print("STRICT RUN")
itermll.runStrict()

print("======================================================================")
print("NONSTRICT RUN")
itermll.run()
