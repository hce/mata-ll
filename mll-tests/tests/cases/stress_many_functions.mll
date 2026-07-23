-- Stress test: 100 top-level functions calling each other

f1 :: Int -> Int
f1 x = x + 1

f2 :: Int -> Int
f2 x = f1 x + 1

f3 :: Int -> Int
f3 x = f2 x + 1

f4 :: Int -> Int
f4 x = f3 x + 1

f5 :: Int -> Int
f5 x = f4 x + 1

f6 :: Int -> Int
f6 x = f5 x + 1

f7 :: Int -> Int
f7 x = f6 x + 1

f8 :: Int -> Int
f8 x = f7 x + 1

f9 :: Int -> Int
f9 x = f8 x + 1

f10 :: Int -> Int
f10 x = f9 x + 1

f11 :: Int -> Int
f11 x = f10 x + 1

f12 :: Int -> Int
f12 x = f11 x + 1

f13 :: Int -> Int
f13 x = f12 x + 1

f14 :: Int -> Int
f14 x = f13 x + 1

f15 :: Int -> Int
f15 x = f14 x + 1

f16 :: Int -> Int
f16 x = f15 x + 1

f17 :: Int -> Int
f17 x = f16 x + 1

f18 :: Int -> Int
f18 x = f17 x + 1

f19 :: Int -> Int
f19 x = f18 x + 1

f20 :: Int -> Int
f20 x = f19 x + 1

f21 :: Int -> Int
f21 x = f20 x + 1

f22 :: Int -> Int
f22 x = f21 x + 1

f23 :: Int -> Int
f23 x = f22 x + 1

f24 :: Int -> Int
f24 x = f23 x + 1

f25 :: Int -> Int
f25 x = f24 x + 1

f26 :: Int -> Int
f26 x = f25 x + 1

f27 :: Int -> Int
f27 x = f26 x + 1

f28 :: Int -> Int
f28 x = f27 x + 1

f29 :: Int -> Int
f29 x = f28 x + 1

f30 :: Int -> Int
f30 x = f29 x + 1

f31 :: Int -> Int
f31 x = f30 x + 1

f32 :: Int -> Int
f32 x = f31 x + 1

f33 :: Int -> Int
f33 x = f32 x + 1

f34 :: Int -> Int
f34 x = f33 x + 1

f35 :: Int -> Int
f35 x = f34 x + 1

f36 :: Int -> Int
f36 x = f35 x + 1

f37 :: Int -> Int
f37 x = f36 x + 1

f38 :: Int -> Int
f38 x = f37 x + 1

f39 :: Int -> Int
f39 x = f38 x + 1

f40 :: Int -> Int
f40 x = f39 x + 1

f41 :: Int -> Int
f41 x = f40 x + 1

f42 :: Int -> Int
f42 x = f41 x + 1

f43 :: Int -> Int
f43 x = f42 x + 1

f44 :: Int -> Int
f44 x = f43 x + 1

f45 :: Int -> Int
f45 x = f44 x + 1

f46 :: Int -> Int
f46 x = f45 x + 1

f47 :: Int -> Int
f47 x = f46 x + 1

f48 :: Int -> Int
f48 x = f47 x + 1

f49 :: Int -> Int
f49 x = f48 x + 1

f50 :: Int -> Int
f50 x = f49 x + 1

f51 :: Int -> Int
f51 x = f50 x + 1

f52 :: Int -> Int
f52 x = f51 x + 1

f53 :: Int -> Int
f53 x = f52 x + 1

f54 :: Int -> Int
f54 x = f53 x + 1

f55 :: Int -> Int
f55 x = f54 x + 1

f56 :: Int -> Int
f56 x = f55 x + 1

f57 :: Int -> Int
f57 x = f56 x + 1

f58 :: Int -> Int
f58 x = f57 x + 1

f59 :: Int -> Int
f59 x = f58 x + 1

f60 :: Int -> Int
f60 x = f59 x + 1

f61 :: Int -> Int
f61 x = f60 x + 1

f62 :: Int -> Int
f62 x = f61 x + 1

f63 :: Int -> Int
f63 x = f62 x + 1

f64 :: Int -> Int
f64 x = f63 x + 1

f65 :: Int -> Int
f65 x = f64 x + 1

f66 :: Int -> Int
f66 x = f65 x + 1

f67 :: Int -> Int
f67 x = f66 x + 1

f68 :: Int -> Int
f68 x = f67 x + 1

f69 :: Int -> Int
f69 x = f68 x + 1

f70 :: Int -> Int
f70 x = f69 x + 1

f71 :: Int -> Int
f71 x = f70 x + 1

f72 :: Int -> Int
f72 x = f71 x + 1

f73 :: Int -> Int
f73 x = f72 x + 1

f74 :: Int -> Int
f74 x = f73 x + 1

f75 :: Int -> Int
f75 x = f74 x + 1

f76 :: Int -> Int
f76 x = f75 x + 1

f77 :: Int -> Int
f77 x = f76 x + 1

f78 :: Int -> Int
f78 x = f77 x + 1

f79 :: Int -> Int
f79 x = f78 x + 1

f80 :: Int -> Int
f80 x = f79 x + 1

f81 :: Int -> Int
f81 x = f80 x + 1

f82 :: Int -> Int
f82 x = f81 x + 1

f83 :: Int -> Int
f83 x = f82 x + 1

f84 :: Int -> Int
f84 x = f83 x + 1

f85 :: Int -> Int
f85 x = f84 x + 1

f86 :: Int -> Int
f86 x = f85 x + 1

f87 :: Int -> Int
f87 x = f86 x + 1

f88 :: Int -> Int
f88 x = f87 x + 1

f89 :: Int -> Int
f89 x = f88 x + 1

f90 :: Int -> Int
f90 x = f89 x + 1

f91 :: Int -> Int
f91 x = f90 x + 1

f92 :: Int -> Int
f92 x = f91 x + 1

f93 :: Int -> Int
f93 x = f92 x + 1

f94 :: Int -> Int
f94 x = f93 x + 1

f95 :: Int -> Int
f95 x = f94 x + 1

f96 :: Int -> Int
f96 x = f95 x + 1

f97 :: Int -> Int
f97 x = f96 x + 1

f98 :: Int -> Int
f98 x = f97 x + 1

f99 :: Int -> Int
f99 x = f98 x + 1

f100 :: Int -> Int
f100 x = f99 x + 1

callMany :: Int -> Int
callMany x = f10 x + f20 x + f50 x + f100 x

main :: IO ()
main = do
    assert (f1 0 == 1) "f1"
    assert (f10 0 == 10) "f10"
    assert (f50 0 == 50) "f50"
    assert (f100 0 == 100) "f100"
    assert (callMany 0 == 180) "callMany"
    assert (f100 100 == 200) "f100 100"
    putStrLn "ok"
