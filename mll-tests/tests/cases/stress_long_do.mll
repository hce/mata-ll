-- Stress test: long do-notation chain with 40 sequential operations

data Ref = Ref Int
    deriving (Show, Eq)

main :: IO ()
main = do
    let x1 = 1
    let x2 = x1 + 2
    let x3 = x2 + 3
    let x4 = x3 + 4
    let x5 = x4 + 5
    let x6 = x5 + 6
    let x7 = x6 + 7
    let x8 = x7 + 8
    let x9 = x8 + 9
    let x10 = x9 + 10
    let x11 = x10 + 11
    let x12 = x11 + 12
    let x13 = x12 + 13
    let x14 = x13 + 14
    let x15 = x14 + 15
    let x16 = x15 + 16
    let x17 = x16 + 17
    let x18 = x17 + 18
    let x19 = x18 + 19
    let x20 = x19 + 20
    let x21 = x20 + 21
    let x22 = x21 + 22
    let x23 = x22 + 23
    let x24 = x23 + 24
    let x25 = x24 + 25
    let x26 = x25 + 26
    let x27 = x26 + 27
    let x28 = x27 + 28
    let x29 = x28 + 29
    let x30 = x29 + 30
    let x31 = x30 + 31
    let x32 = x31 + 32
    let x33 = x32 + 33
    let x34 = x33 + 34
    let x35 = x34 + 35
    let x36 = x35 + 36
    let x37 = x36 + 37
    let x38 = x37 + 38
    let x39 = x38 + 39
    let x40 = x39 + 40
    assert (x40 == 820) "sum 1..40"
    assert (x1 == 1) "x1"
    assert (x20 == 210) "x20"
    putStrLn "phase 1 ok"
    let y1 = x40 + 1
    let y2 = x40 + 2
    let y3 = x40 + 3
    let y4 = x40 + 4
    let y5 = x40 + 5
    let y6 = x40 + 6
    let y7 = x40 + 7
    let y8 = x40 + 8
    let y9 = x40 + 9
    let y10 = x40 + 10
    assert (y10 == 830) "y10"
    putStrLn "ok"
