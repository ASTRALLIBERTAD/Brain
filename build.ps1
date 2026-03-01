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
Write-Host "2. compiler\main.brn"
$choice = Read-Host "Choose an option (1 or 2)"

if ($choice -eq "1") {
    $SourceDir = "examples"
} elseif ($choice -eq "2") {
    $SourceDir = "compiler"
} else {
    Write-Host "Invalid selection. Exiting." -ForegroundColor Red
    exit
}
# Path where your compiler saves the IR
$SourcePath = "$SourceDir\main.brn"
$InputIR    = "$SourceDir\main.ll"
$OptIR      = "$SourceDir\main_opt.ll"
$ExeOut     = "$SourceDir\main_final.exe"

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
