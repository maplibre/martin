FROM node:23-alpine as builder

WORKDIR /usr/src/app

COPY package.json .
COPY yarn.lock .
RUN yarn install

COPY . .
RUN yarn run build

CMD ["yarn", "run", "preview"]
