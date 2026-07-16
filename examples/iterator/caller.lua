local myCoroutine = coroutine.wrap(function()
		local callme
		callme = function()
			local x = 0
			coroutine.yield(y)
			x = x + 1
			callme()
		end
		callme()
	end)

function myiterator()
	return myCoroutine, true, 0
end

local callee = require "callee"
callee.run()

