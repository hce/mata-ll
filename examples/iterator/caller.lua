local myCoroutine = coroutine.wrap(function()
		local callme
		callme = function(x)
			coroutine.yield(x)
			callme(x + 1)
		end
		callme(0)
	end)

function myiterator()
	return myCoroutine, true, 0
end

local callee = require "callee"
callee.run()

