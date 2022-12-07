DROP SCHEMA IF EXISTS "MixedCase" CASCADE;
CREATE SCHEMA "MixedCase";

CREATE TABLE "MixedCase"."MixPoints"("Gid" SERIAL PRIMARY KEY, "TABLE" TEXT, "Geom" GEOMETRY(POINT, 4326));

INSERT INTO "MixedCase"."MixPoints"
    SELECT
        generate_series(1, 10000) as id,
        md5(random()::text) as "TABLE",
        (
            ST_DUMP(ST_GENERATEPOINTS(ST_GEOMFROMTEXT('POLYGON ((-180 90, 180 90, 180 -90, -180 -90, -180 90))', 4326), 10000))
        ).Geom;

CREATE INDEX ON "MixedCase"."MixPoints" USING GIST("Geom");
