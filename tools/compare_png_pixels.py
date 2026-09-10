import hashlib
import json
import sys

from PIL import Image


def main() -> int:
    if len(sys.argv) != 3:
        return 2
    with Image.open(sys.argv[1]) as left_image:
        left = left_image.convert("RGBA")
    with Image.open(sys.argv[2]) as right_image:
        right = right_image.convert("RGBA")
    if left.size != right.size:
        print(
            json.dumps(
                {
                    "ok": False,
                    "sameDimensions": False,
                    "leftSize": left.size,
                    "rightSize": right.size,
                }
            )
        )
        return 2
    left_bytes = left.tobytes()
    right_bytes = right.tobytes()
    differences = [
        abs(left_value - right_value)
        for left_value, right_value in zip(left_bytes, right_bytes)
    ]
    changed_channels = sum(value != 0 for value in differences)
    total_channels = len(differences)
    same_fraction = (
        1.0
        if total_channels == 0
        else (total_channels - changed_channels) / total_channels
    )
    mean_difference = (
        0.0
        if total_channels == 0
        else sum(differences) / total_channels
    )
    result = {
        "ok": True,
        "sameDimensions": True,
        "width": left.size[0],
        "height": left.size[1],
        "sameChannelFraction": round(same_fraction, 8),
        "meanAbsoluteDifference": round(mean_difference, 8),
        "maximumChannelDifference": max(differences, default=0),
        "leftPixelSha256": hashlib.sha256(left_bytes).hexdigest(),
        "rightPixelSha256": hashlib.sha256(right_bytes).hexdigest(),
        "exactPixels": left_bytes == right_bytes,
    }
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
