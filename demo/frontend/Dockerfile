FROM node:alpine@sha256:c8d96e95e88f08f814af06415db9cfd5ab4ebcdf40721327ff2172ff25cfb997 AS builder

WORKDIR /usr/src/app

COPY package.json .
COPY yarn.lock .
RUN yarn install

COPY . .
RUN yarn run build

FROM nginx:alpine@sha256:1d13701a5f9f3fb01aaa88cef2344d65b6b5bf6b7d9fa4cf0dca557a8d7702ba
COPY nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /usr/src/app/dist /usr/share/nginx/html
