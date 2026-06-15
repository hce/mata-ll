-- Stress test: 100 top-level functions calling each other

f1 :: Integer -> Integer
f1 x = x + 1

f2 :: Integer -> Integer
f2 x = f1 x + 1

f3 :: Integer -> Integer
f3 x = f2 x + 1

f4 :: Integer -> Integer
f4 x = f3 x + 1

f5 :: Integer -> Integer
f5 x = f4 x + 1

f6 :: Integer -> Integer
f6 x = f5 x + 1

f7 :: Integer -> Integer
f7 x = f6 x + 1

f8 :: Integer -> Integer
f8 x = f7 x + 1

f9 :: Integer -> Integer
f9 x = f8 x + 1

f10 :: Integer -> Integer
f10 x = f9 x + 1

f11 :: Integer -> Integer
f11 x = f10 x + 1

f12 :: Integer -> Integer
f12 x = f11 x + 1

f13 :: Integer -> Integer
f13 x = f12 x + 1

f14 :: Integer -> Integer
f14 x = f13 x + 1

f15 :: Integer -> Integer
f15 x = f14 x + 1

f16 :: Integer -> Integer
f16 x = f15 x + 1

f17 :: Integer -> Integer
f17 x = f16 x + 1

f18 :: Integer -> Integer
f18 x = f17 x + 1

f19 :: Integer -> Integer
f19 x = f18 x + 1

f20 :: Integer -> Integer
f20 x = f19 x + 1

f21 :: Integer -> Integer
f21 x = f20 x + 1

f22 :: Integer -> Integer
f22 x = f21 x + 1

f23 :: Integer -> Integer
f23 x = f22 x + 1

f24 :: Integer -> Integer
f24 x = f23 x + 1

f25 :: Integer -> Integer
f25 x = f24 x + 1

f26 :: Integer -> Integer
f26 x = f25 x + 1

f27 :: Integer -> Integer
f27 x = f26 x + 1

f28 :: Integer -> Integer
f28 x = f27 x + 1

f29 :: Integer -> Integer
f29 x = f28 x + 1

f30 :: Integer -> Integer
f30 x = f29 x + 1

f31 :: Integer -> Integer
f31 x = f30 x + 1

f32 :: Integer -> Integer
f32 x = f31 x + 1

f33 :: Integer -> Integer
f33 x = f32 x + 1

f34 :: Integer -> Integer
f34 x = f33 x + 1

f35 :: Integer -> Integer
f35 x = f34 x + 1

f36 :: Integer -> Integer
f36 x = f35 x + 1

f37 :: Integer -> Integer
f37 x = f36 x + 1

f38 :: Integer -> Integer
f38 x = f37 x + 1

f39 :: Integer -> Integer
f39 x = f38 x + 1

f40 :: Integer -> Integer
f40 x = f39 x + 1

f41 :: Integer -> Integer
f41 x = f40 x + 1

f42 :: Integer -> Integer
f42 x = f41 x + 1

f43 :: Integer -> Integer
f43 x = f42 x + 1

f44 :: Integer -> Integer
f44 x = f43 x + 1

f45 :: Integer -> Integer
f45 x = f44 x + 1

f46 :: Integer -> Integer
f46 x = f45 x + 1

f47 :: Integer -> Integer
f47 x = f46 x + 1

f48 :: Integer -> Integer
f48 x = f47 x + 1

f49 :: Integer -> Integer
f49 x = f48 x + 1

f50 :: Integer -> Integer
f50 x = f49 x + 1

f51 :: Integer -> Integer
f51 x = f50 x + 1

f52 :: Integer -> Integer
f52 x = f51 x + 1

f53 :: Integer -> Integer
f53 x = f52 x + 1

f54 :: Integer -> Integer
f54 x = f53 x + 1

f55 :: Integer -> Integer
f55 x = f54 x + 1

f56 :: Integer -> Integer
f56 x = f55 x + 1

f57 :: Integer -> Integer
f57 x = f56 x + 1

f58 :: Integer -> Integer
f58 x = f57 x + 1

f59 :: Integer -> Integer
f59 x = f58 x + 1

f60 :: Integer -> Integer
f60 x = f59 x + 1

f61 :: Integer -> Integer
f61 x = f60 x + 1

f62 :: Integer -> Integer
f62 x = f61 x + 1

f63 :: Integer -> Integer
f63 x = f62 x + 1

f64 :: Integer -> Integer
f64 x = f63 x + 1

f65 :: Integer -> Integer
f65 x = f64 x + 1

f66 :: Integer -> Integer
f66 x = f65 x + 1

f67 :: Integer -> Integer
f67 x = f66 x + 1

f68 :: Integer -> Integer
f68 x = f67 x + 1

f69 :: Integer -> Integer
f69 x = f68 x + 1

f70 :: Integer -> Integer
f70 x = f69 x + 1

f71 :: Integer -> Integer
f71 x = f70 x + 1

f72 :: Integer -> Integer
f72 x = f71 x + 1

f73 :: Integer -> Integer
f73 x = f72 x + 1

f74 :: Integer -> Integer
f74 x = f73 x + 1

f75 :: Integer -> Integer
f75 x = f74 x + 1

f76 :: Integer -> Integer
f76 x = f75 x + 1

f77 :: Integer -> Integer
f77 x = f76 x + 1

f78 :: Integer -> Integer
f78 x = f77 x + 1

f79 :: Integer -> Integer
f79 x = f78 x + 1

f80 :: Integer -> Integer
f80 x = f79 x + 1

f81 :: Integer -> Integer
f81 x = f80 x + 1

f82 :: Integer -> Integer
f82 x = f81 x + 1

f83 :: Integer -> Integer
f83 x = f82 x + 1

f84 :: Integer -> Integer
f84 x = f83 x + 1

f85 :: Integer -> Integer
f85 x = f84 x + 1

f86 :: Integer -> Integer
f86 x = f85 x + 1

f87 :: Integer -> Integer
f87 x = f86 x + 1

f88 :: Integer -> Integer
f88 x = f87 x + 1

f89 :: Integer -> Integer
f89 x = f88 x + 1

f90 :: Integer -> Integer
f90 x = f89 x + 1

f91 :: Integer -> Integer
f91 x = f90 x + 1

f92 :: Integer -> Integer
f92 x = f91 x + 1

f93 :: Integer -> Integer
f93 x = f92 x + 1

f94 :: Integer -> Integer
f94 x = f93 x + 1

f95 :: Integer -> Integer
f95 x = f94 x + 1

f96 :: Integer -> Integer
f96 x = f95 x + 1

f97 :: Integer -> Integer
f97 x = f96 x + 1

f98 :: Integer -> Integer
f98 x = f97 x + 1

f99 :: Integer -> Integer
f99 x = f98 x + 1

f100 :: Integer -> Integer
f100 x = f99 x + 1

callMany :: Integer -> Integer
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
