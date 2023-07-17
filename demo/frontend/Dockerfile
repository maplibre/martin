FROM node:18-alpine as builder

WORKDIR /usr/src/app
COPY . .
RUN yarn install
RUN yarn run build
