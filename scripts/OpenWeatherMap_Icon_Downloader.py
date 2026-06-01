#!/usr/bin/env python3
"""
OpenWeatherMap Icon Downloader

This script downloads OpenWeatherMap weather icons, resizes them to suitable
dimensions for ESP32 displays.
"""

import importlib.util
from pathlib import Path

import requests
from PIL import Image

# Base URL for OpenWeatherMap Icons
BASE_URL = "https://openweathermap.org/img/wn/"

# All available icon codes (day and night versions) from OpenWeatherMap
ICON_CODES = [
    "01",  # Clear sky
    "02",  # Few clouds
    "03",  # Scattered clouds
    "04",  # Broken clouds
    "09",  # Shower rain
    "10",  # Rain
    "11",  # Thunderstorm
    "13",  # Snow
    "50",  # Mist/Fog
]

# Additional icons not directly from OWM weather codes but used in the project
ADDITIONAL_ICONS = [
    "wifi_on",
    "wifi_off",
]


def download_icon(icon_name: str, output_dir: Path, size: str = "@2x") -> bool:
    if icon_name in ADDITIONAL_ICONS:
        # Zeilenumbruch im Print löst E501
        print(
            f"Skipping download for non-OWM icon: {icon_name}. "
            "Please provide manually if needed."
        )
        return False

    url = f"{BASE_URL}{icon_name}{size}.png"
    output_path = output_dir / f"{icon_name}.png"

    try:
        response = requests.get(url, timeout=10)
        response.raise_for_status()
        with open(output_path, "wb") as f:
            f.write(response.content)
        print(f"  ✓ Saved: {output_path}")
    except requests.exceptions.RequestException as e:
        print(f"  ✗ Error downloading {icon_name}: {e}")
        return False
    # BLE001: Ersetze Exception durch konkretere Fehler oder unterdrücke es
    except RuntimeError as e:
        print(f"  ✗ Unexpected error for {icon_name}: {e}")
        return False
    else:
        # TRY300: return True gehört in den else-Block
        return True


def resize_icons(
    input_dir: Path, output_dir: Path, target_size: tuple = (48, 48)
) -> None:
    """
    Resizes all PNG icons in the input directory to a target size.

    Args:
        input_dir (Path): Directory containing the original icons.
        output_dir (Path): Directory to save the resized icons.
        target_size (tuple): Target size as (width, height) in pixels.
    """
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"\nResizing icons to {target_size[0]}x{target_size[1]}px for ESP32...")

    for png_file in input_dir.glob("*.png"):
        try:
            img = Image.open(png_file)

            # Resize to target size with high quality resampling
            img_resized = img.resize(target_size, Image.Resampling.LANCZOS)

            # WICHTIG: RGB ohne Alpha erzwingen
            img_resized = img_resized.convert("RGBA")

            output_path = output_dir / png_file.name

            img_resized.save(
                output_path, format="PNG", optimize=False, compress_level=0
            )

            print(f"  ✓ {png_file.name} → {output_path}")

        except (OSError, ValueError) as e:  # Spezifische Fehler statt Exception
            print(f"  ✗ Error resizing {png_file.name}: {e}")


def main() -> None:
    """
    Main function to orchestrate the icon downloading and preparation process.
    """
    print("=" * 60)
    print("OpenWeatherMap Icon Downloader for ESP32")
    print("=" * 60)

    # Define directories
    base_output_dir = Path("icons_png")
    original_size_dir = base_output_dir / "original"
    esp32_target_dir = (
        base_output_dir / "48x48"
    )  # Renamed to be more generic for final output for LVGL

    original_size_dir.mkdir(
        parents=True, exist_ok=True
    )  # Ensure original directory exists

    # Step 1: Download Icons
    print("\n[1/2] Downloading icons from OpenWeatherMap...")
    downloaded_count = 0

    all_icon_names = []
    for code in ICON_CODES:
        all_icon_names.append(f"{code}d")  # Day version
        all_icon_names.append(f"{code}n")  # Night version
    # Adding additional icons to the list for processing
    all_icon_names.extend(ADDITIONAL_ICONS)

    for icon_name in all_icon_names:
        if download_icon(icon_name, original_size_dir):
            downloaded_count += 1
        elif icon_name in ADDITIONAL_ICONS:
            # Create dummy files for additional icons if not downloaded,
            # for resizing later
            dummy_path = original_size_dir / f"{icon_name}.png"
            if not dummy_path.exists():
                print(
                    f"  Creating dummy PNG for {icon_name}. "
                    f"Please replace with actual icon if needed."
                )
                # Create a simple white square as a placeholder
                Image.new("RGB", (100, 100), color="white").save(dummy_path)
                downloaded_count += 1  # Count dummy as processed

    print(f"\n✓ {downloaded_count} icons processed (downloaded or dummy created).")

    # Step 2: Resize Icons for ESP32
    print(f"\n[2/2] Resizing icons to 48x48px and saving to '{esp32_target_dir}'...")
    resize_icons(original_size_dir, esp32_target_dir, target_size=(48, 48))

    # Summary
    print("\n" + "=" * 60)
    print("✓ Icon Preparation Complete!")
    print("=" * 60)
    print(f"\nYour icons are prepared in the '{esp32_target_dir}' directory.")
    print("\nNext Steps:")
    print(f"  1. Review '{esp32_target_dir}' for your ready-to-convert PNG icons.")
    print("  2. Run 'convert_icons.py' (located in the same 'scripts' folder).")
    print("     It will convert these PNGs into LVGL-compatible '.bin' files.")
    print(
        "  3. Copy the resulting '.bin' files from the 'icons' "
        "folder to your ESP32's '/icons' directory."
    )
    print("=" * 60)


if __name__ == "__main__":
    import importlib.util
    import sys

    # Check if Pillow (PIL) is installed
    if importlib.util.find_spec("PIL") is None:
        print("ERROR: Pillow (PIL) is not installed!")
        print("Please run: pip install Pillow")
        sys.exit(1)

    try:
        main()
    except KeyboardInterrupt:
        print("\nAborted by user.")
        sys.exit(0)
