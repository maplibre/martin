FROM node:20-alpine as builder

WORKDIR /usr/src/app

COPY package.json .
COPY yarn.lock .
RUN yarn install

COPY . .
CMD yarn run build
