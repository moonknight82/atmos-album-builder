# FFmpeg notice

Atmos Album Builder includes FFmpeg and FFprobe 7.1.4, built from the
unmodified source published at:

https://ffmpeg.org/releases/ffmpeg-7.1.4.tar.xz

These executables run as separate processes and were configured without GPL or
non-free components. The build reports License: LGPL version 2.1 or later.
The complete license is included in FFmpeg-COPYING.LGPLv2.1.txt.

Build configuration:

    --arch=arm64 --disable-shared --enable-static --disable-doc --disable-debug
    --disable-network --disable-autodetect --disable-avdevice --disable-postproc
    --disable-everything --enable-ffmpeg --enable-ffprobe --enable-avcodec
    --enable-avformat --enable-avfilter --enable-swscale --enable-swresample
    --enable-videotoolbox --enable-zlib
    --enable-demuxer=mov,concat,ffmetadata,image2,matroska
    --enable-muxer=matroska,streamhash,image2
    --enable-decoder=eac3,ac3,mjpeg,png,webp,tiff,bmp,h264,hevc
    --enable-encoder=mjpeg,h264_videotoolbox
    --enable-parser=ac3,mjpeg,png,h264,hevc
    --enable-filter=scale,pad,setsar
    --enable-protocol=file,pipe
