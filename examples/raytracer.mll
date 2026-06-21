-- A tiny raytracer: a self-checking compiler stress test.
--
-- Target: Number arithmetic, record-heavy vector/color types with field
-- access, and deeply nested let bindings in the intersection/shading math
-- (the part cheapness/laziness analysis must reason about).
--
-- Scene is hardcoded (a [Sphere] plus a point light and an origin camera).
-- Output is a PPM (P3) image written to stdout, so:
--     lua examples/raytracer.lua > out.ppm
-- produces a viewable image. Checks are SILENT on success (they only error
-- on failure) so they never corrupt the image on stdout.
--
-- Oracle: tolerance-based geometric invariants (a known ray hits a known
-- sphere at t=4; a normalized vector has length 1; perpendicular dot is 0)
-- plus sentinel pixels (the center pixel sees the red sphere; a corner is
-- background). A broken result -> error -> the program (and test) fails.

-- Lua FFI: math + an Integer->Number coercion (tonumber is ~identity at
-- runtime since both are Lua numbers; the type just changes).
sqrtN   :: Number -> LuaPure "math.sqrt" Number
absN    :: Number -> LuaPure "math.abs" Number
floorN  :: Number -> LuaPure "math.floor" Integer
intToNum :: Integer -> LuaPure "tonumber" Number

neg :: Number -> Number
neg x = 0.0 - x

maxN :: Number -> Number -> Number
maxN a b = if a > b then a else b

-- ── vectors / records ───────────────────────────────────────────────────

data Vec3 = Vec3 { vx :: Number, vy :: Number, vz :: Number }

vadd :: Vec3 -> Vec3 -> Vec3
vadd a b = Vec3 (vx a + vx b) (vy a + vy b) (vz a + vz b)

vsub :: Vec3 -> Vec3 -> Vec3
vsub a b = Vec3 (vx a - vx b) (vy a - vy b) (vz a - vz b)

vscale :: Number -> Vec3 -> Vec3
vscale s a = Vec3 (s * vx a) (s * vy a) (s * vz a)

vdot :: Vec3 -> Vec3 -> Number
vdot a b = vx a * vx b + vy a * vy b + vz a * vz b

vlen :: Vec3 -> Number
vlen a = sqrtN (vdot a a)

vnorm :: Vec3 -> Vec3
vnorm a = vscale (1.0 / vlen a) a

-- ── scene types ─────────────────────────────────────────────────────────

data Ray = Ray { rorig :: Vec3, rdir :: Vec3 }

data Sphere = Sphere { scenter :: Vec3, sradius :: Number, scolor :: Vec3 }

-- A hit carries: distance t, point, surface normal, surface color.
data Hit = Miss | Hit Number Vec3 Vec3 Vec3

-- ── intersection (nested let math) ──────────────────────────────────────

intersect :: Ray -> Sphere -> Hit
intersect ray sph =
  let oc   = vsub (rorig ray) (scenter sph)
      b    = vdot oc (rdir ray)
      c    = vdot oc oc - sradius sph * sradius sph
      disc = b * b - c
  in if disc < 0.0
       then Miss
       else
         let t = neg b - sqrtN disc
         in if t < 0.001
              then Miss
              else
                let p = vadd (rorig ray) (vscale t (rdir ray))
                    n = vnorm (vsub p (scenter sph))
                in Hit t p n (scolor sph)

closer :: Hit -> Hit -> Hit
closer a Miss = a
closer Miss b = b
closer (Hit ta pa na ca) (Hit tb pb nb cb) =
  if ta < tb then Hit ta pa na ca else Hit tb pb nb cb

nearestHit :: [Sphere] -> Ray -> Hit
nearestHit scene ray = foldr (\s acc -> closer (intersect ray s) acc) Miss scene

-- ── scene ────────────────────────────────────────────────────────────────

scene :: [Sphere]
scene =
  [ Sphere (Vec3 0.0 0.0 (neg 5.0)) 1.0 (Vec3 0.9 0.2 0.2)
  , Sphere (Vec3 (neg 2.2) 0.0 (neg 6.0)) 1.0 (Vec3 0.2 0.4 0.9)
  , Sphere (Vec3 2.2 0.0 (neg 6.0)) 1.0 (Vec3 0.2 0.8 0.3)
  , Sphere (Vec3 0.0 (neg 101.0) (neg 5.0)) 100.0 (Vec3 0.6 0.6 0.6)
  ]

lightPos :: Vec3
lightPos = Vec3 5.0 5.0 0.0

background :: Vec3
background = Vec3 0.05 0.05 0.10

-- ── shading ──────────────────────────────────────────────────────────────

shade :: Hit -> Vec3
shade Miss = background
shade (Hit t p n col) =
  let toL    = vnorm (vsub lightPos p)
      diff   = maxN 0.0 (vdot n toL)
      shadow = case nearestHit scene (Ray (vadd p (vscale 0.001 n)) toL) of
                 Miss -> 1.0
                 _    -> 0.25
      factor = 0.15 + 0.85 * diff * shadow
  in vscale factor col

-- ── camera ───────────────────────────────────────────────────────────────

width :: Integer
width = 80

height :: Integer
height = 60

aspect :: Number
aspect = intToNum width / intToNum height

primaryRay :: Integer -> Integer -> Ray
primaryRay px py =
  let u  = (intToNum px + 0.5) / intToNum width
      v  = (intToNum py + 0.5) / intToNum height
      sx = (2.0 * u - 1.0) * aspect
      sy = 1.0 - 2.0 * v
  in Ray (Vec3 0.0 0.0 0.0) (vnorm (Vec3 sx sy (neg 1.0)))

colorAt :: Integer -> Integer -> Vec3
colorAt px py = shade (nearestHit scene (primaryRay px py))

-- ── quantization / output ────────────────────────────────────────────────

clamp01 :: Number -> Number
clamp01 c = if c < 0.0 then 0.0 else if c > 1.0 then 1.0 else c

quant :: Number -> Integer
quant c = floorN (clamp01 c * 255.0)

emitPixels :: [Vec3] -> IO ()
emitPixels []     = return ()
emitPixels (c:cs) = do
  putStrLn (show (quant (vx c)) <> " " <> show (quant (vy c)) <> " " <> show (quant (vz c)))
  emitPixels cs

allPixels :: [Vec3]
allPixels = concatMap (\py -> map (\px -> colorAt px py) (enumFromTo 0 (width - 1))) (enumFromTo 0 (height - 1))

-- ── self-checks (silent on success) ───────────────────────────────────────

check :: Bool -> String -> IO ()
check True  _   = return ()
check False msg = error ("raytracer check failed: " <> msg)

near :: Number -> Number -> Bool
near a b = absN (a - b) < 0.001

-- distance t of a hit (or -1 for a miss), for the geometric oracle
hitT :: Hit -> Number
hitT Miss             = neg 1.0
hitT (Hit t _ _ _)    = t

main :: IO ()
main = do
  -- Geometric invariants (the strong, deterministic oracle).
  let downRay = Ray (Vec3 0.0 0.0 0.0) (Vec3 0.0 0.0 (neg 1.0))
  let frontSphere = Sphere (Vec3 0.0 0.0 (neg 5.0)) 1.0 (Vec3 1.0 1.0 1.0)
  check (near (hitT (intersect downRay frontSphere)) 4.0) "ray hits sphere at t=4"
  check (near (vlen (vnorm (Vec3 3.0 0.0 0.0))) 1.0)      "normalized length is 1"
  check (near (vdot (Vec3 1.0 0.0 0.0) (Vec3 0.0 1.0 0.0)) 0.0) "perp dot is 0"

  -- Sentinel pixels: center sees the red sphere; a corner is background.
  let centerC = colorAt (floorN (intToNum width / 2.0)) (floorN (intToNum height / 2.0))
  check (quant (vx centerC) > quant (vz centerC)) "center pixel is reddish (R > B)"
  check (quant (vx centerC) > 40)                 "center pixel is lit"
  let cornerC = colorAt 0 0
  check (quant (vx cornerC) < 25) "corner pixel is background"

  -- Emit the PPM image to stdout (header + one RGB triple per line).
  putStrLn "P3"
  putStrLn (show width <> " " <> show height)
  putStrLn "255"
  emitPixels allPixels
