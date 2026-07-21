local tracker
local load_succ, load_err_msg = pcall(function()
	tracker = require "tracker"
end)
if not load_succ then
	print([[

Before you can run this file, you need to compile the tracker.mll to
Lua using the mata-ll compiler.

    cargo run -- tracker.mll

Then run ctracker.lua while setting the cwd to the path it and
tracker.lua both reside in.

]])

	print("Error: " .. tostring(load_err_msg))
	return
end

local function mk_process_chunk(handle, verbose)
	local count = 1
	return function(chunk)
		if verbose then
			io.stdout:write(string.format("\rChunk %d of %d bytes", count, #chunk))
			io.stdout:flush()
			count = count + 1
		end
		-- We could be writing to a sound card here,
		-- to stdio, piped to sox, or to a file
		handle:write(chunk)
	end
end

if #arg < 1 then
	print([[IT tracker file player
======================================================================

Demonstrates passing Lua functions to mata-ll to recieve chunks of
data back. If called with one argument, decodes the file and passes it
to sox. Assumes sox to be present in the current path.

If called with two arguments, will convert the IT tracker file to a
raw pcm file that can be played back with sox using:

    sox -t raw -r 44100 -e signed -b 16 -c 2 -L filename.raw -d
]])
	return
end

local input = io.open(arg[1], "rb")
local output, err, errno

if arg[2] then
	output, err, errno = io.open(arg[2], "w")
else
	output, err, errno = io.popen("sox -t raw -r 44100 -e signed -b 16 -c 2 -L - -d", "w")
end

if not output then
	print("Unable to open output file: " .. tostring(err))
	print("Error code: " .. tostring(errno))
end

local chunk_swallower = mk_process_chunk(output, #arg > 1)

local no_loop = true
local tracker_data = input:read("a")
tracker.play(chunk_swallower, tracker_data, no_loop)

print()
print("Finished already.")
