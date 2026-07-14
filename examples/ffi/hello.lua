function handle_me(parameters)
	local operation = parameters.operation
	local operands = parameters.operands
	print(operands[1], operands[2])
	local res = { }
	if operation == "+" then
		res.value = operands[1] + operands[2]
		res.success = true
	elseif operation == "-" then
		res.value = operands[1] - operands[2]
		res.success = true
	elseif operation == "*" then
		res.value = operands[1] * operands[2]
		res.success = true
	elseif operation == "/" then
		res.value = operands[1] / operands[2]
		res.success = true
	elseif operation == "^" then
		res.value = math.pow(operands[1], operands[2])
		res.success = true
	else
		res.success = false
	end
	return res
end

local callhello = require "callhello"
callhello.doit()

