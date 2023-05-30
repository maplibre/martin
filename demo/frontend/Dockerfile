FROM node:18-alpine as builder

WORKDIR /usr/src/app
COPY . .
RUN npm i
RUN npm run build