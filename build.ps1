# 1. Build the Brain Compiler (Rust)
Write-Host "--- [1/3] Building Brain Compiler (Release) ---" -ForegroundColor Cyan
cargo build --release

if ($LASTEXITCODE -ne 0) {
    Write-Host "Cargo build failed." -ForegroundColor Red
    exit
}

# 2. Select Source File
Write-Host "`n--- [2/3] Select Source File ---" -ForegroundColor Cyan
Write-Host "1. examples\main.brn"
Write-Host "2. examples\game\main.brn   (Crypts of Brain - dungeon crawler)"
Write-Host "3. compiler\main.brn        (Building on progress)"
$choice = Read-Host "Choose an option (1, 2, or 3)"

if ($choice -eq "1") {
    $SourcePath = "examples\main.brn"
    $InputIR    = "examples\main.ll"
    $OptIR      = "examples\main_opt.ll"
    $ExeOut     = "examples\main_final.exe"
} elseif ($choice -eq "2") {
    $SourcePath = "examples\game\main.brn"
    $InputIR    = "examples\game\main.ll"
    $OptIR      = "examples\game\main_opt.ll"
    $ExeOut     = "examples\game\main_final.exe"
} elseif ($choice -eq "3") {
    $SourcePath = "compiler\main.brn"
    $InputIR    = "compiler\main.ll"
    $OptIR      = "compiler\main_opt.ll"
    $ExeOut     = "compiler\main_final.exe"
} else {
    Write-Host "Invalid selection. Exiting." -ForegroundColor Red
    exit
}

# 3. Run the Compiler
Write-Host "Compiling $SourcePath..." -ForegroundColor Yellow
& "target\release\brain.exe" $SourcePath

if ($LASTEXITCODE -ne 0) {
    Write-Host "Brain compiler failed." -ForegroundColor Red
    exit
}

# 4. Optimize and Link (LLVM)
Write-Host "`n--- [3/3] LLVM Pipeline ---" -ForegroundColor Cyan

if (Test-Path $InputIR) {
    # Optimize IR
    Write-Host "Running O3 Optimization..." -ForegroundColor Gray
    clang -S -emit-llvm -O3 $InputIR -o $OptIR -Wno-override-module

    # Link to Final EXE
    Write-Host "Linking to $ExeOut..." -ForegroundColor Gray
    clang -O3 $OptIR -o $ExeOut -lkernel32 -luser32 -Wno-override-module

    if ($LASTEXITCODE -eq 0) {
        Write-Host "Success!" -ForegroundColor Green
        Write-Host "Running Program:`n" -ForegroundColor Magenta
        Write-Host "----------------"
        & "$ExeOut"
    } else {
        Write-Host "Clang linking failed." -ForegroundColor Red
    }
} else {
    Write-Host "Error: Could not find $InputIR. Check where your Rust compiler saves the .ll file." -ForegroundColor Red
}
