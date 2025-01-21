FROM ghcr.io/webassembly/wasi-sdk:wasi-sdk-25 AS build-env

RUN apt-get update
RUN apt-get install -y git
RUN mkdir /build

FROM build-env AS freetype
WORKDIR /build
RUN git clone https://gitlab.freedesktop.org/freetype/freetype
WORKDIR /build/freetype
RUN ./autogen.sh
ENV CFLAGS="-target wasm32-wasip1 -fwasm-exceptions -mllvm -wasm-enable-sjlj"
ENV LDFLAGS="$CFLAGS"
RUN ./configure --with-brotli=no --with-bzip2=no --with-zlib=no --disable-mmap --host wasm32-wasip1
RUN make -j $(nproc)
RUN make install

FROM build-env AS harfbuzz
WORKDIR /build
RUN git clone https://github.com/harfbuzz/harfbuzz
WORKDIR /build/harfbuzz
RUN apt-get install -y meson pkg-config ragel gtk-doc-tools gcc g++

COPY --from=freetype /usr/local/lib /usr/local/lib
COPY --from=freetype /usr/local/include /usr/local/include
ENV CFLAGS="-target wasm32-wasip1 -fwasm-exceptions -mllvm -wasm-enable-sjlj"
ENV LIBRARY_PATH="/usr/local/lib:${LIBRARY_PATH}"
ENV PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:${PKG_CONFIG_PATH}"
ENV PKG_CONFIG="/bin/pkg-config"

RUN $CXX src/harfbuzz.cc -I /usr/local/include/freetype2 $CFLAGS -c -DHB_NO_MMAP -DHB_NO_MT -fno-exceptions -DHAVE_FREETYPE -DHAVE_FT_GET_TRANSFORM -DHAVE_FT_DONE_MMVAR -o harfbuzz.o
RUN $AR q libharfbuzz.a harfbuzz.o

FROM scratch
COPY --from=freetype /build/freetype/objs/.libs /
COPY --from=freetype /build/freetype/objs/libfreetype.la /
COPY --from=harfbuzz /build/harfbuzz/libharfbuzz.a /
