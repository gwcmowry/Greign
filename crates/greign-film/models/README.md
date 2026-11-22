# Bundled upright-detection model

`yunet-2023mar.onnx` is OpenCV Zoo's full-precision YuNet model. It detects faces
and five landmarks (eyes, nose, mouth corners). Greign uses these outputs only
for confidence-based quarter-turn suggestions, with an explicit abstention when
faces or the winning orientation are unclear. It does not identify people.

Source: https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet

Download: https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx

SHA-256: `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4`

Size: 232,589 bytes. License: MIT; see `YUNET-LICENSE` in this directory.
The binary embeds the model. CPU inference uses Tract with no runtime download
or Python requirement. An optional Linux ONNX Runtime CUDA backend loads locally
installed native libraries; both backends use the same preprocessing, output
decoding, landmark checks, and decision thresholds. See `docs/native-workflow.md`
for the optional one-time GPU setup. Inference never uploads photographs.
The four orientation passes run independently in a Rayon pool capped at four
CPU workers. A one-worker reference is available for timing and parity checks.

Input is a positive sRGB picture, resized proportionally to a 640-pixel long
edge and zero-padded on the right/bottom to 640 × 640. Channels are raw 0–255 BGR
floats in NCHW order. Output decoding follows the model's three feature strides
(8, 16, 32); overlapping detections are suppressed before landmark plausibility
and orientation scores are evaluated. Confidence scores are not probabilities
that a rotation is correct. Partial/tilted/reclining people and non-human faces
can be ambiguous, so a strong detection and a margin over competing quarter
turns are both required.
