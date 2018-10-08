FROM node:10-alpine as builder
LABEL maintainer="Andrey Bakhvalov<bakhvalov.andrey@gmail.com>"

WORKDIR /usr/src/app
COPY package.json /usr/src/app/package.json
RUN yarn
COPY . .
RUN yarn run build

FROM nginx:alpine
COPY nginx.conf /etc/nginx/nginx.conf
COPY --from=builder /usr/src/app/build /usr/share/nginx/html
