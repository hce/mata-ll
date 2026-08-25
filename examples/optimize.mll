-- This example is not really interesting to run,
-- its intention is to demonstrate compile time
-- optimization. Compile it with mll to Lua and
-- scroll down to the bottom of the file. You should
-- see a routine that says print("23").
abc :: Int
abc = 17

def :: Int -> Int
def x = x + 1

ghi :: Int
ghi = abc + def 5

main :: IO ()
main = print ghi
