function my_iter()
	return coroutine.wrap(function()
		for i = 1, 10 do
			print("Yielding", i)
			coroutine.yield(i)
		end
	end)
end

function runaway_loop_iterator()
	return coroutine.wrap(function()
		local callme = function(next_fun, c)
			print("    runaway loop returning", c)
			coroutine.yield(c)
			next_fun(next_fun, c + 1)
		end
		callme(callme, 0)
	end)
end

local itermll = require "itermll"
print("======================================================================")
print("STRICT RUN")
itermll.runStrict()

print()
print("======================================================================")
print("NONSTRICT RUN")
itermll.run()

print()
print("======================================================================")
print("TAKE n ITEMS ONLY:")
-- runawayLoopIterator is a single shared list, so successive runPartly
-- calls resume the same coroutine rather than restart it: take 10 pulls
-- 0..9, take 20 pulls only the fresh 10..19, take 30 only 20..29.
print("Take 10 items only:")
itermll.runPartly(10)
print("Take 20 items only:")
itermll.runPartly(20)
print("Take 30 items only:")
itermll.runPartly(30)

