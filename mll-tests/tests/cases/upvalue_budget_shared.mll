-- Routing a capture through a table keeps the variable shared (O11): a
-- routed local is replaced by its slot at its declaration, every
-- assignment and every read, so a closure sees what the enclosing
-- function writes after creating it. Shapes: 61 let bindings of an ST
-- block, two of which the inline array read forces in place before the
-- lambda over all of them is made; an inner lambda capturing 61
-- grandparent bindings through its parent; a lambda whose captures are
-- 50 bindings plus the runtime's show and string helpers; and a lambda
-- over 55 parameters of the enclosing function.

rebound61 :: Int -> Int
rebound61 n = runST (do
    arr <- newSTArray 70 n
    let
        x1 = n * 1 + 1
        x2 = n * 2 + 2
        x3 = n * 3 + 3
        x4 = n * 4 + 4
        x5 = n * 5 + 5
        x6 = n * 6 + 6
        x7 = n * 7 + 7
        x8 = n * 8 + 8
        x9 = n * 9 + 9
        x10 = n * 10 + 10
        x11 = n * 11 + 11
        x12 = n * 12 + 12
        x13 = n * 13 + 13
        x14 = n * 14 + 14
        x15 = n * 15 + 15
        x16 = n * 16 + 16
        x17 = n * 17 + 17
        x18 = n * 18 + 18
        x19 = n * 19 + 19
        x20 = n * 20 + 20
        x21 = n * 21 + 21
        x22 = n * 22 + 22
        x23 = n * 23 + 23
        x24 = n * 24 + 24
        x25 = n * 25 + 25
        x26 = n * 26 + 26
        x27 = n * 27 + 27
        x28 = n * 28 + 28
        x29 = n * 29 + 29
        x30 = n * 30 + 30
        x31 = n * 31 + 31
        x32 = n * 32 + 32
        x33 = n * 33 + 33
        x34 = n * 34 + 34
        x35 = n * 35 + 35
        x36 = n * 36 + 36
        x37 = n * 37 + 37
        x38 = n * 38 + 38
        x39 = n * 39 + 39
        x40 = n * 40 + 40
        x41 = n * 41 + 41
        x42 = n * 42 + 42
        x43 = n * 43 + 43
        x44 = n * 44 + 44
        x45 = n * 45 + 45
        x46 = n * 46 + 46
        x47 = n * 47 + 47
        x48 = n * 48 + 48
        x49 = n * 49 + 49
        x50 = n * 50 + 50
        x51 = n * 51 + 51
        x52 = n * 52 + 52
        x53 = n * 53 + 53
        x54 = n * 54 + 54
        x55 = n * 55 + 55
        x56 = n * 56 + 56
        x57 = n * 57 + 57
        x58 = n * 58 + 58
        x59 = n * 59 + 59
        x60 = n * 60 + 60
        x61 = n * 61 + 61
    v <- readSTArray arr x3
    w <- readSTArray arr x1
    let g = \q -> q + v + w + x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61
    pure (sum (map g [1, 2, 3])))

nested61 :: Int -> [Int] -> [[Int]]
nested61 n ks = map (\a -> map (\b -> a * b + x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61) ks) ks
  where
    x1 = n * 1 + 1
    x2 = n * 2 + 2
    x3 = n * 3 + 3
    x4 = n * 4 + 4
    x5 = n * 5 + 5
    x6 = n * 6 + 6
    x7 = n * 7 + 7
    x8 = n * 8 + 8
    x9 = n * 9 + 9
    x10 = n * 10 + 10
    x11 = n * 11 + 11
    x12 = n * 12 + 12
    x13 = n * 13 + 13
    x14 = n * 14 + 14
    x15 = n * 15 + 15
    x16 = n * 16 + 16
    x17 = n * 17 + 17
    x18 = n * 18 + 18
    x19 = n * 19 + 19
    x20 = n * 20 + 20
    x21 = n * 21 + 21
    x22 = n * 22 + 22
    x23 = n * 23 + 23
    x24 = n * 24 + 24
    x25 = n * 25 + 25
    x26 = n * 26 + 26
    x27 = n * 27 + 27
    x28 = n * 28 + 28
    x29 = n * 29 + 29
    x30 = n * 30 + 30
    x31 = n * 31 + 31
    x32 = n * 32 + 32
    x33 = n * 33 + 33
    x34 = n * 34 + 34
    x35 = n * 35 + 35
    x36 = n * 36 + 36
    x37 = n * 37 + 37
    x38 = n * 38 + 38
    x39 = n * 39 + 39
    x40 = n * 40 + 40
    x41 = n * 41 + 41
    x42 = n * 42 + 42
    x43 = n * 43 + 43
    x44 = n * 44 + 44
    x45 = n * 45 + 45
    x46 = n * 46 + 46
    x47 = n * 47 + 47
    x48 = n * 48 + 48
    x49 = n * 49 + 49
    x50 = n * 50 + 50
    x51 = n * 51 + 51
    x52 = n * 52 + 52
    x53 = n * 53 + 53
    x54 = n * 54 + 54
    x55 = n * 55 + 55
    x56 = n * 56 + 56
    x57 = n * 57 + 57
    x58 = n * 58 + 58
    x59 = n * 59 + 59
    x60 = n * 60 + 60
    x61 = n * 61 + 61

helpers50 :: Int -> IO ()
helpers50 n = mapM_ (\q -> putStrLn (show (q + x1) <> "," <> show x2 <> "," <> show x3 <> "," <> show x4 <> "," <> show x5 <> "," <> show x6 <> "," <> show x7 <> "," <> show x8 <> "," <> show x9 <> "," <> show x10 <> "," <> show x11 <> "," <> show x12 <> "," <> show x13 <> "," <> show x14 <> "," <> show x15 <> "," <> show x16 <> "," <> show x17 <> "," <> show x18 <> "," <> show x19 <> "," <> show x20 <> "," <> show x21 <> "," <> show x22 <> "," <> show x23 <> "," <> show x24 <> "," <> show x25 <> "," <> show x26 <> "," <> show x27 <> "," <> show x28 <> "," <> show x29 <> "," <> show x30 <> "," <> show x31 <> "," <> show x32 <> "," <> show x33 <> "," <> show x34 <> "," <> show x35 <> "," <> show x36 <> "," <> show x37 <> "," <> show x38 <> "," <> show x39 <> "," <> show x40 <> "," <> show x41 <> "," <> show x42 <> "," <> show x43 <> "," <> show x44 <> "," <> show x45 <> "," <> show x46 <> "," <> show x47 <> "," <> show x48 <> "," <> show x49 <> "," <> show x50)) [1, 2]
  where
    x1 = n * 1 + 1
    x2 = n * 2 + 2
    x3 = n * 3 + 3
    x4 = n * 4 + 4
    x5 = n * 5 + 5
    x6 = n * 6 + 6
    x7 = n * 7 + 7
    x8 = n * 8 + 8
    x9 = n * 9 + 9
    x10 = n * 10 + 10
    x11 = n * 11 + 11
    x12 = n * 12 + 12
    x13 = n * 13 + 13
    x14 = n * 14 + 14
    x15 = n * 15 + 15
    x16 = n * 16 + 16
    x17 = n * 17 + 17
    x18 = n * 18 + 18
    x19 = n * 19 + 19
    x20 = n * 20 + 20
    x21 = n * 21 + 21
    x22 = n * 22 + 22
    x23 = n * 23 + 23
    x24 = n * 24 + 24
    x25 = n * 25 + 25
    x26 = n * 26 + 26
    x27 = n * 27 + 27
    x28 = n * 28 + 28
    x29 = n * 29 + 29
    x30 = n * 30 + 30
    x31 = n * 31 + 31
    x32 = n * 32 + 32
    x33 = n * 33 + 33
    x34 = n * 34 + 34
    x35 = n * 35 + 35
    x36 = n * 36 + 36
    x37 = n * 37 + 37
    x38 = n * 38 + 38
    x39 = n * 39 + 39
    x40 = n * 40 + 40
    x41 = n * 41 + 41
    x42 = n * 42 + 42
    x43 = n * 43 + 43
    x44 = n * 44 + 44
    x45 = n * 45 + 45
    x46 = n * 46 + 46
    x47 = n * 47 + 47
    x48 = n * 48 + 48
    x49 = n * 49 + 49
    x50 = n * 50 + 50

params55 :: Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> Int -> [Int] -> [Int]
params55 p1 p2 p3 p4 p5 p6 p7 p8 p9 p10 p11 p12 p13 p14 p15 p16 p17 p18 p19 p20 p21 p22 p23 p24 p25 p26 p27 p28 p29 p30 p31 p32 p33 p34 p35 p36 p37 p38 p39 p40 p41 p42 p43 p44 p45 p46 p47 p48 p49 p50 p51 p52 p53 p54 p55 ks = map (\k -> k + p1 + p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9 + p10 + p11 + p12 + p13 + p14 + p15 + p16 + p17 + p18 + p19 + p20 + p21 + p22 + p23 + p24 + p25 + p26 + p27 + p28 + p29 + p30 + p31 + p32 + p33 + p34 + p35 + p36 + p37 + p38 + p39 + p40 + p41 + p42 + p43 + p44 + p45 + p46 + p47 + p48 + p49 + p50 + p51 + p52 + p53 + p54 + p55) ks

main :: IO ()
main = do
    let n = length (filter even [1 .. 6])
    print (rebound61 n)
    print (nested61 n [1, 2])
    helpers50 n
    print (params55 (n + 1) (n + 2) (n + 3) (n + 4) (n + 5) (n + 6) (n + 7) (n + 8) (n + 9) (n + 10) (n + 11) (n + 12) (n + 13) (n + 14) (n + 15) (n + 16) (n + 17) (n + 18) (n + 19) (n + 20) (n + 21) (n + 22) (n + 23) (n + 24) (n + 25) (n + 26) (n + 27) (n + 28) (n + 29) (n + 30) (n + 31) (n + 32) (n + 33) (n + 34) (n + 35) (n + 36) (n + 37) (n + 38) (n + 39) (n + 40) (n + 41) (n + 42) (n + 43) (n + 44) (n + 45) (n + 46) (n + 47) (n + 48) (n + 49) (n + 50) (n + 51) (n + 52) (n + 53) (n + 54) (n + 55) [1, 2, 3])
