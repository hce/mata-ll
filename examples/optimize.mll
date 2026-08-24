abc :: Int
abc = 17

def :: Int -> Int
def x = x + 1

ghi :: Int
ghi = abc + def 5

main :: IO ()
main = print ghi
