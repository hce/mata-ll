import LIO
import LOS
import LMath
import ByteString
import LString

data Wav = Wav
    { wavChannels   :: Int
    , wavRate       :: Int
    , wavBytesPer   :: Int
    , wavBlockAlign :: Int
    , wavDataOffset :: Int
    , wavFrames     :: Int
    , wavFullRange  :: Int
    , wavHalfRange  :: Int
    , wavScale      :: Number
    }

data Complex = Complex Number Number

data Screen = Screen { screenCols :: Int, screenRows :: Int }

fftSize :: Int
fftSize = 2048

framesPerSecond :: Number
framesPerSecond = 30.0

lowestHz :: Number
lowestHz = 30.0

floorDb :: Number
floorDb = -72.0

complex :: Number -> Number -> Complex
complex re im = re `seq` im `seq` Complex re im

cAdd :: Complex -> Complex -> Complex
cAdd (Complex a b) (Complex c d) = complex (a + c) (b + d)

cSub :: Complex -> Complex -> Complex
cSub (Complex a b) (Complex c d) = complex (a - c) (b - d)

cMul :: Complex -> Complex -> Complex
cMul (Complex a b) (Complex c d) = complex (a * c - b * d) (a * d + b * c)

rotateClockwise :: Complex -> Complex
rotateClockwise (Complex x y) = complex y (negate x)

cMagnitude :: Complex -> Number
cMagnitude (Complex a b) = sqrt (a * a + b * b)

toNumber :: Int -> Number
toNumber = fromIntegral

prependReversed :: [a] -> [a] -> [a]
prependReversed [] acc = acc
prependReversed (x : xs) acc = prependReversed xs (x : acc)

deinterleave :: [a] -> ([a], [a])
deinterleave = go [] []
  where
    go evens odds (a : b : rest) = go (a : evens) (b : odds) rest
    go evens odds rest = (prependReversed evens rest, reverse odds)

evenIndexed :: [a] -> [a]
evenIndexed = go []
  where
    go acc (a : _ : rest) = go (a : acc) rest
    go acc rest = prependReversed acc rest

unitRoots :: Int -> [Complex]
unitRoots n =
    [ complex (cos angle) (sin angle)
    | k <- [0 .. n `div` 2 - 1]
    , let angle = -2.0 * pi * toNumber k / toNumber n
    ]

butterflies :: [Complex] -> [Complex] -> [Complex] -> [Complex]
butterflies = go [] []
  where
    go sums diffs (r : rs) (e : es) (o : os) =
        let t = cMul r o
            s = cAdd e t
            d = cSub e t
        in s `seq` d `seq` go (s : sums) (d : diffs) rs es os
    go sums diffs _ _ _ = prependReversed sums (reverse diffs)

rootLevels :: [Complex] -> [[Complex]]
rootLevels roots = takeWhile (not . null) (iterate evenIndexed roots)

fft :: [[Complex]] -> [Complex] -> [Complex]
fft _ [x] = [x]
fft _ [a, b] = [cAdd a b, cSub a b]
fft _ [a, b, c, d] =
    let sumAC = cAdd a c
        diffAC = cSub a c
        sumBD = cAdd b d
        rotated = rotateClockwise (cSub b d)
    in [cAdd sumAC sumBD, cAdd diffAC rotated, cSub sumAC sumBD, cSub diffAC rotated]
fft (roots : childLevels) xs =
    let (evens, odds) = deinterleave xs
    in butterflies roots (fft childLevels evens) (fft childLevels odds)
fft [] xs = xs

hannWindow :: Int -> [Number]
hannWindow n = [ 0.5 - 0.5 * cos (2.0 * pi * toNumber i / toNumber (n - 1)) | i <- [0 .. n - 1] ]

littleEndian :: ByteString -> Int -> Int -> Int
littleEndian bytes offset width = go (width - 1) 0
  where
    go i acc
        | i < 0 = acc
        | otherwise = go (i - 1) (acc * 256 + bsIndex bytes (offset + i))

pow2 :: Int -> Int
pow2 k = if k <= 0 then 1 else 2 * pow2 (k - 1)

signedSample :: Wav -> Int -> Int
signedSample wav raw
    | wavBytesPer wav == 1 = raw - wavHalfRange wav
    | raw >= wavHalfRange wav = raw - wavFullRange wav
    | otherwise = raw

makeWav :: Int -> Int -> Int -> Int -> Int -> Int -> Wav
makeWav channels rate bytesPer blockAlign offset frames =
    let full = pow2 (8 * bytesPer)
        half = full `div` 2
    in Wav channels rate bytesPer blockAlign offset frames full half
        (1.0 / (toNumber half * toNumber channels))

asciiAt :: ByteString -> Int -> Int -> String
asciiAt bytes offset len = bsToString (bsSub bytes offset len)

readBytes :: FileHandle -> Int -> IO ByteString
readBytes handle count =
    if count <= 0 then pure bsEmpty else fmap bsFromString (fReadN handle count)

requireBytes :: FileHandle -> Int -> String -> IO ByteString
requireBytes handle count what = do
    bytes <- readBytes handle count
    if bsLength bytes < count then error ("truncated WAV file: " <> what) else pure bytes

parseWav :: FileHandle -> IO Wav
parseWav handle = do
    riff <- requireBytes handle 12 "RIFF header"
    if asciiAt riff 0 4 /= "RIFF" || asciiAt riff 8 4 /= "WAVE"
        then error "not a RIFF/WAVE file"
        else scanChunks handle Nothing

scanChunks :: FileHandle -> Maybe (Int, Int, Int, Int) -> IO Wav
scanChunks handle format = do
    header <- requireBytes handle 8 "chunk header"
    let chunkId = asciiAt header 0 4
        chunkSize = littleEndian header 4 4
        padded = chunkSize + chunkSize `mod` 2
    case chunkId of
        "fmt " -> do
            body <- requireBytes handle chunkSize "fmt chunk"
            _ <- fSeek handle "cur" (padded - chunkSize)
            fmt <- parseFormat body
            scanChunks handle (Just fmt)
        "data" -> case format of
            Nothing -> error "data chunk precedes fmt chunk"
            Just (channels, rate, bits, blockAlign) -> do
                offset <- fSeek handle "cur" 0
                end <- fSeek handle "end" 0
                let available = min chunkSize (end - offset)
                pure (makeWav channels rate (bits `div` 8) blockAlign offset (available `div` blockAlign))
        _ -> do
            _ <- fSeek handle "cur" padded
            scanChunks handle format

parseFormat :: ByteString -> IO (Int, Int, Int, Int)
parseFormat body = do
    let tag = littleEndian body 0 2
        channels = littleEndian body 2 2
        rate = littleEndian body 4 4
        blockAlign = littleEndian body 12 2
        bits = littleEndian body 14 2
        extensiblePcm = tag == 65534 && bsLength body >= 26 && littleEndian body 24 2 == 1
    if tag /= 1 && not extensiblePcm
        then error ("unsupported WAV encoding " <> show tag <> " (integer PCM only)")
        else if bits `mod` 8 /= 0 || bits < 8 || bits > 32
            then error ("unsupported sample width " <> show bits <> " bits")
            else if channels < 1 || rate < 1 || blockAlign < channels * (bits `div` 8)
                then error "inconsistent fmt chunk"
                else pure (channels, rate, bits, blockAlign)

monoFrame :: Wav -> ByteString -> Int -> Number
monoFrame wav bytes frame = toNumber (go (wavChannels wav - 1) 0) * wavScale wav
  where
    base = frame * wavBlockAlign wav
    width = wavBytesPer wav
    go c acc
        | c < 0 = acc
        | otherwise = go (c - 1) (acc + signedSample wav (littleEndian bytes (base + c * width) width))

decodeFrames :: Wav -> ByteString -> Int -> [Number]
decodeFrames wav bytes count = go (count - 1) []
  where
    go i acc
        | i < 0 = acc
        | otherwise = let sample = monoFrame wav bytes i in sample `seq` go (i - 1) (sample : acc)

readWindow :: FileHandle -> Wav -> Int -> IO [Number]
readWindow handle wav firstFrame = do
    let start = max 0 firstFrame
        leading = start - firstFrame
        wanted = fftSize - leading
        count = max 0 (min wanted (wavFrames wav - start))
    _ <- fSeek handle "set" (wavDataOffset wav + start * wavBlockAlign wav)
    bytes <- readBytes handle (count * wavBlockAlign wav)
    let frames = bsLength bytes `div` wavBlockAlign wav
        samples = decodeFrames wav bytes frames
    pure (replicate leading 0.0 <> samples <> replicate (fftSize - leading - frames) 0.0)

pairUp :: [Number] -> [Complex]
pairUp (a : b : rest) = complex a b : pairUp rest
pairUp _ = []

mirrored :: [a] -> [a]
mirrored (x : rest) = x : reverse rest
mirrored [] = []

spectrum :: [Complex] -> [[Complex]] -> [Number] -> [Number] -> [Number]
spectrum roots halfRootLevels window samples =
    let packed = fft halfRootLevels (pairUp (zipWith (*) window samples))
    in zipWith3 unpackBin roots packed (mirrored packed)

unpackBin :: Complex -> Complex -> Complex -> Number
unpackBin root (Complex a b) (Complex c d) =
    let evenPart = complex ((a + c) * 0.5) ((b - d) * 0.5)
        oddPart = complex ((b + d) * 0.5) ((c - a) * 0.5)
    in cMagnitude (cAdd evenPart (cMul root oddPart))

bandEdges :: Int -> Int -> [(Int, Int)]
bandEdges rate columns =
    let nyquist = toNumber rate / 2.0
        highest = min 18000.0 nyquist
        binOf hz = floor (hz * toNumber fftSize / toNumber rate)
        edge c = lowestHz * exp (log (highest / lowestHz) * toNumber c / toNumber columns)
        band c =
            let lo = max 1 (binOf (edge c))
                hi = max (lo + 1) (binOf (edge (c + 1)))
            in (lo, min (fftSize `div` 2) hi)
    in map band [0 .. columns - 1]

bandLevels :: [(Int, Int)] -> [Number] -> [Number]
bandLevels edges magnitudes = go edges 0 magnitudes
  where
    go [] _ _ = []
    go ((lo, hi) : rest) cursor remaining =
        let aligned = drop (lo - cursor) remaining
            peak = foldl max 0.0 (take (hi - lo) aligned)
        in decibels peak : go rest lo aligned

decibels :: Number -> Number
decibels magnitude =
    let reference = toNumber fftSize / 4.0
        ratio = max 1.0e-9 (magnitude / reference)
    in 20.0 * log ratio / log 10.0

barCell :: Number -> Int -> String
barCell height row
    | height >= toNumber row = "#"
    | height >= toNumber row - 0.5 = "."
    | otherwise = " "

rowColour :: Int -> Int -> String
rowColour rows row
    | row * 4 > rows * 3 = "\27[31m"
    | row * 2 > rows = "\27[33m"
    | otherwise = "\27[32m"

renderBars :: Int -> [Number] -> String
renderBars rows levels =
    let heights = [ max 0.0 (toNumber rows * (1.0 - db / floorDb)) | db <- levels ]
        line row = rowColour rows row <> mconcat (map (\h -> barCell h row) heights) <> "\27[0m\27[K\n"
    in mconcat (map line [rows, rows - 1 .. 1])

frequencyLabel :: Number -> String
frequencyLabel hz =
    if hz >= 1000.0
        then show (round (hz / 100.0) `div` 10) <> "k"
        else show (round hz)

axis :: Int -> Int -> String
axis rate columns =
    let highest = min 18000.0 (toNumber rate / 2.0)
        edge c = lowestHz * exp (log (highest / lowestHz) * toNumber c / toNumber columns)
        go c
            | c >= columns = ""
            | c `mod` 12 == 0 =
                let label = frequencyLabel (edge c)
                in label <> go (c + strLen label)
            | otherwise = " " <> go (c + 1)
    in go 0 <> "\27[K\n"

clockText :: Number -> String
clockText seconds =
    let whole = max 0 (floor seconds)
        secs = whole `mod` 60
    in show (whole `div` 60) <> ":" <> (if secs < 10 then "0" else "") <> show secs

wallClock :: IO Number
wallClock = do
    opened <- pOpen "date +%s.%N" "r"
    case opened of
        Left err -> error ("cannot read the wall clock: " <> err)
        Right pipe -> do
            line <- fReadLine pipe
            fClose pipe
            case line of
                Just text -> pure (read text)
                Nothing -> error "date printed nothing"

sleepSeconds :: Number -> IO ()
sleepSeconds seconds = do
    completed <- execute ("sleep " <> show seconds)
    when (not completed) (error "interrupted!")

screenSize :: IO Screen
screenSize = do
    opened <- pOpen "stty size 2>/dev/null" "r"
    case opened of
        Left _ -> pure fallback
        Right pipe -> do
            line <- fReadLine pipe
            fClose pipe
            pure (maybe fallback parse line)
  where
    fallback = Screen 80 24
    parse text = case splitSpace (strToInts text) of
        (rows, cols) | rows > 0 && cols > 0 -> Screen cols rows
        _ -> fallback

splitSpace :: [Int] -> (Int, Int)
splitSpace codes =
    let digitsOf = foldl (\acc c -> acc * 10 + (c - 48)) 0
        first = takeWhile isDigitCode codes
        second = takeWhile isDigitCode (drop 1 (dropWhile isDigitCode codes))
    in if null first || null second then (0, 0) else (digitsOf first, digitsOf second)

shellQuote :: String -> String
shellQuote path = "'" <> mconcat (map quoteByte (strToInts path)) <> "'"
  where
    quoteByte 39 = "'\\''"
    quoteByte c = strChar c

data Player = Player
    { playerHandle  :: FileHandle
    , playerWav     :: Wav
    , playerName    :: String
    , playerRoots   :: [Complex]
    , playerLevels  :: [[Complex]]
    , playerWindow  :: [Number]
    , playerEdges   :: [(Int, Int)]
    , playerRows    :: Int
    , playerColumns :: Int
    }

analysisStart :: Wav -> Int -> Int
analysisStart wav centre = max 0 (min (wavFrames wav - fftSize) (centre - fftSize `div` 2))

drawFrame :: Player -> Number -> IO ()
drawFrame player elapsed = do
    let wav = playerWav player
        rate = wavRate wav
        centre = floor (elapsed * toNumber rate)
        duration = toNumber (wavFrames wav) / toNumber rate
    samples <- readWindow (playerHandle player) wav (analysisStart wav centre)
    let levels = bandLevels (playerEdges player) (spectrum (playerRoots player) (playerLevels player) (playerWindow player) samples)
        status = strSub (playerName player <> "  " <> clockText elapsed <> " / " <> clockText duration
            <> "  " <> show rate <> " Hz  " <> show (wavChannels wav) <> " ch") 1 (playerColumns player) <> "\27[K\n"
    putStr ("\27[H" <> status <> renderBars (playerRows player) levels <> axis rate (playerColumns player))
    flushStdout

playLoop :: Player -> Number -> IO ()
playLoop player started = do
    now <- wallClock
    let elapsed = now - started
        wav = playerWav player
        duration = toNumber (wavFrames wav) / toNumber (wavRate wav)
    if elapsed >= duration
        then pure ()
        else do
            drawFrame player elapsed
            finished <- wallClock
            let nextFrame = (toNumber (floor (elapsed * framesPerSecond)) + 1.0) / framesPerSecond
                pause = started + nextFrame - finished
            when (pause > 0.002) (sleepSeconds pause)
            playLoop player started

main :: IO ()
main = do
    args <- getArgs
    outcome <- try (case args of
        [path] -> run path
        _ -> error "usage: spectrum <file.wav>")
    case outcome of
        Right () -> pure ()
        Left err -> do
            putStrLn ("spectrum: " <> err)
            exit 1

startPlayback :: String -> IO String
startPlayback path = do
    opened <- pOpen ("afplay " <> shellQuote path <> " >/dev/null 2>&1 & echo $!") "r"
    case opened of
        Left err -> error ("could not start afplay: " <> err)
        Right pipe -> do
            line <- fReadLine pipe
            fClose pipe
            case line of
                Just pid | not (null (strToInts pid)) && all isDigitCode (strToInts pid) -> pure pid
                _ -> error "could not start afplay"

baseName :: String -> String
baseName path = mconcat (map strChar (reverse (takeWhile (/= 47) (reverse (strToInts path)))))

isDigitCode :: Int -> Bool
isDigitCode c = c >= 48 && c <= 57

stopPlayback :: String -> IO ()
stopPlayback pid = do
    _ <- execute ("kill " <> pid <> " 2>/dev/null")
    pure ()

run :: String -> IO ()
run path = do
    opened <- fOpen path "rb"
    case opened of
        Left err -> error err
        Right handle -> do
            wav <- parseWav handle
            screen <- screenSize
            let columns = max 16 (screenCols screen - 1)
                rows = max 4 (screenRows screen - 3)
                roots = unitRoots fftSize
                player = Player handle wav (baseName path) roots (rootLevels (evenIndexed roots)) (hannWindow fftSize)
                    (bandEdges (wavRate wav) columns) rows columns
            pid <- startPlayback path
            putStr "\27[2J"
            begin <- wallClock
            outcome <- try (playLoop player begin)
            stopPlayback pid
            putStr "\27[0m\n"
            fClose handle
            case outcome of
                Right () -> pure ()
                Left err
                    | strSub err (-12) (-1) == "interrupted!" -> exit 130
                    | otherwise -> error err
