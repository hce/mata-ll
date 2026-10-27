-- A function literal never captures more enclosing locals than the host
-- allows (O11). LuaJIT refuses to load a function with more than 60
-- upvalues, and a lambda or a thunk over a where group, a let group or a
-- do block of that many bindings captured every one of them; the Lua-level
-- upvalue pass routes the captures of such a function through a table of
-- the declaring scope. Shapes: a lambda over 70 where bindings, a do
-- block of 64 binds under mapM_, a list of 64 where bindings consumed
-- lazily, a lambda over 65 let bindings, a thunk (not a lambda) over 70
-- let bindings, the same thunk declared before the bindings it reads (its
-- closure is made before they are assigned, so a routed binding must stay
-- one shared cell), and the budget boundary: a lambda over 49 bindings
-- keeps its upvalues, one over 50 is routed.

where70 :: Int -> [Int] -> [Int]
where70 n ks = map (\k -> x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61 + x62 + x63 + x64 + x65 + x66 + x67 + x68 + x69 + x70 + k) ks
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
    x62 = n * 62 + 62
    x63 = n * 63 + 63
    x64 = n * 64 + 64
    x65 = n * 65 + 65
    x66 = n * 66 + 66
    x67 = n * 67 + 67
    x68 = n * 68 + 68
    x69 = n * 69 + 69
    x70 = n * 70 + 70

do64 :: IO ()
do64 = do
    v0 <- pure 0
    v1 <- pure 1
    v2 <- pure 2
    v3 <- pure 3
    v4 <- pure 4
    v5 <- pure 5
    v6 <- pure 6
    v7 <- pure 7
    v8 <- pure 8
    v9 <- pure 9
    v10 <- pure 10
    v11 <- pure 11
    v12 <- pure 12
    v13 <- pure 13
    v14 <- pure 14
    v15 <- pure 15
    v16 <- pure 16
    v17 <- pure 17
    v18 <- pure 18
    v19 <- pure 19
    v20 <- pure 20
    v21 <- pure 21
    v22 <- pure 22
    v23 <- pure 23
    v24 <- pure 24
    v25 <- pure 25
    v26 <- pure 26
    v27 <- pure 27
    v28 <- pure 28
    v29 <- pure 29
    v30 <- pure 30
    v31 <- pure 31
    v32 <- pure 32
    v33 <- pure 33
    v34 <- pure 34
    v35 <- pure 35
    v36 <- pure 36
    v37 <- pure 37
    v38 <- pure 38
    v39 <- pure 39
    v40 <- pure 40
    v41 <- pure 41
    v42 <- pure 42
    v43 <- pure 43
    v44 <- pure 44
    v45 <- pure 45
    v46 <- pure 46
    v47 <- pure 47
    v48 <- pure 48
    v49 <- pure 49
    v50 <- pure 50
    v51 <- pure 51
    v52 <- pure 52
    v53 <- pure 53
    v54 <- pure 54
    v55 <- pure 55
    v56 <- pure 56
    v57 <- pure 57
    v58 <- pure 58
    v59 <- pure 59
    v60 <- pure 60
    v61 <- pure 61
    v62 <- pure 62
    v63 <- pure 63
    mapM_ (\q -> print (q + v0 + v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10 + v11 + v12 + v13 + v14 + v15 + v16 + v17 + v18 + v19 + v20 + v21 + v22 + v23 + v24 + v25 + v26 + v27 + v28 + v29 + v30 + v31 + v32 + v33 + v34 + v35 + v36 + v37 + v38 + v39 + v40 + v41 + v42 + v43 + v44 + v45 + v46 + v47 + v48 + v49 + v50 + v51 + v52 + v53 + v54 + v55 + v56 + v57 + v58 + v59 + v60 + v61 + v62 + v63)) [1, 2]

pick :: [Int] -> Int -> Int
pick xs i = xs !! i

pick64 :: Int -> Int -> Int
pick64 n a = pick (map (+ 1) [x1, x2, x3, x4, x5, x6, x7, x8, x9, x10, x11, x12, x13, x14, x15, x16, x17, x18, x19, x20, x21, x22, x23, x24, x25, x26, x27, x28, x29, x30, x31, x32, x33, x34, x35, x36, x37, x38, x39, x40, x41, x42, x43, x44, x45, x46, x47, x48, x49, x50, x51, x52, x53, x54, x55, x56, x57, x58, x59, x60, x61, x62, x63, x64]) a
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
    x62 = n * 62 + 62
    x63 = n * 63 + 63
    x64 = n * 64 + 64

let65 :: Int -> [Int] -> [Int]
let65 n ks =
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
        x62 = n * 62 + 62
        x63 = n * 63 + 63
        x64 = n * 64 + 64
        x65 = n * 65 + 65
    in map (\k -> k + x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61 + x62 + x63 + x64 + x65) ks

thunk70 :: Int -> Int
thunk70 n =
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
        x62 = n * 62 + 62
        x63 = n * 63 + 63
        x64 = n * 64 + 64
        x65 = n * 65 + 65
        x66 = n * 66 + 66
        x67 = n * 67 + 67
        x68 = n * 68 + 68
        x69 = n * 69 + 69
        x70 = n * 70 + 70
        big = x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61 + x62 + x63 + x64 + x65 + x66 + x67 + x68 + x69 + x70
    in if n > 0 then big else 0

thunkFirst70 :: Int -> Int
thunkFirst70 n =
    let
        big = x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + x51 + x52 + x53 + x54 + x55 + x56 + x57 + x58 + x59 + x60 + x61 + x62 + x63 + x64 + x65 + x66 + x67 + x68 + x69 + x70
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
        x62 = n * 62 + 62
        x63 = n * 63 + 63
        x64 = n * 64 + 64
        x65 = n * 65 + 65
        x66 = n * 66 + 66
        x67 = n * 67 + 67
        x68 = n * 68 + 68
        x69 = n * 69 + 69
        x70 = n * 70 + 70
    in if n > 0 then big else 0

atBudget49 :: Int -> [Int] -> [Int]
atBudget49 n ks = map (\k -> x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + k) ks
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

overBudget50 :: Int -> [Int] -> [Int]
overBudget50 n ks = map (\k -> x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9 + x10 + x11 + x12 + x13 + x14 + x15 + x16 + x17 + x18 + x19 + x20 + x21 + x22 + x23 + x24 + x25 + x26 + x27 + x28 + x29 + x30 + x31 + x32 + x33 + x34 + x35 + x36 + x37 + x38 + x39 + x40 + x41 + x42 + x43 + x44 + x45 + x46 + x47 + x48 + x49 + x50 + k) ks
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

main :: IO ()
main = do
    let n = length (filter even [1 .. 6])
    print (sum (where70 n [1, 2, 3]))
    do64
    print (pick64 n 0)
    print (pick64 n 63)
    print (sum (map (pick64 n) [0 .. 63]))
    print (let65 n [1, 2, 3])
    print (thunk70 n)
    print (thunk70 (n - 3))
    print (thunkFirst70 n)
    print (sum (atBudget49 n [1, 2, 3]))
    print (sum (overBudget50 n [1, 2, 3]))
