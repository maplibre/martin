PKG_NAME="martin"
CONFIG_DIR="/usr/local/etc/martin.d"
CONFIG_FILE="/usr/local/etc/martin.d/martin.env"



if [ ! -f $CONFIG_FILE ]
then
  echo "===> Creating config in ${CONFIG_FILE}"
  echo "# martin config file" > $CONFIG_FILE
  echo 'FOO="bar"' >> $CONFIG_FILE
  chmod 0444 $CONFIG_FILE
fi
